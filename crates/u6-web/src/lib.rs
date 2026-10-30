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
//! next one holds the whole save. A Save with something remembered also writes the remembered
//! sight (spec 18, `saveload::SIGHT_FILE`), stored and laid over as any save file.
//!
//! Voices (spec 09): with [`Game::set_voices`] on, the engine's speech tap keeps what NPCs say
//! in conversations, and the page takes it after each frame with [`Game::take_speech`] for its
//! on-device synthesis; off (the default), the tap keeps nothing and no frame changes either way.
//!
//! Remembered sight (spec 18): with [`Game::set_sight_memory`] on, the map window shows the cells
//! in view that were seen and are out of sight now in dark grey ([`u6_core::Io::set_sight_memory`]);
//! off (the default, the original's black), the frames are those without it.
//!
//! Smooth motion (spec 20): with [`Game::set_motion`] on, the engine hands over the scene of each
//! picture of the map window it publishes ([`u6_core::Engine::set_motion`]), and while a slide runs the
//! map shows as the slides have it at the presentation clock ([`Game::presentation_ms`]: the
//! engine's time and the pacer's carried fraction): the leader's step and every object one cell
//! from where it was move there over 18 PAL frames of that clock (`u6_render::motion`). With HD off
//! each present draws it into the RGBA frame. With HD on the HD frame stays the engine's: the
//! slide's layers (the static map with a cell round it, the cursor and border, each mover and what
//! covers it) are composed once a publication, the static and frame layers as their changes from
//! those the page holds, and each present only places them ([`Game::slide`]); the page draws them
//! over its HD frame's map. The page presents every display frame while [`Game::sliding`]. Off
//! (the default), the frames are the engine's.
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
//! [`MAX_GAME_FILES_BYTES`] for the game files the intake produces, [`MAX_SAVE_FILE_BYTES`]
//! (the remembered sight's file: `u6_core::gfx::cache::MAX_SIGHT_FILE_BYTES`) and
//! [`MAX_SAVE_BYTES`] for the stored save (a file over them is left out, and the start's report
//! says so).

mod zipscan;

use std::collections::BTreeMap;
use u6_render::motion::footsteps::{Footsteps, Pattern, Pose};

use std::rc::Rc;

use u6_core::gfx::cache::MAX_SIGHT_FILE_BYTES;
use u6_core::gfx::motion::Shot;
use u6_core::hd::{HdView, PackCounts};
use u6_core::objects::{BLOCK_COUNT, MAX_RECORDS};
use u6_core::pacer::{self, Pacer, Speed};
use u6_core::saveload::{MemStore, OBJLIST_FILE, SIGHT_FILE, attach, block_file, changed_since};
use u6_core::sound::AudioDriver;
use u6_core::{Engine, Files, Framebuffer, InputEvent, Key};
use u6_intake::{InputFile, intake};
use u6_render::hd::{self, Scale};
use u6_render::motion::{Presenter, Scene, View, slides_shown};
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
/// The remembered sight's file (spec 18, [`SIGHT_FILE`]) is beside it, under its own limit
/// ([`save_file_limit`]), and not counted in it.
pub const MAX_SAVE_BYTES: usize = (1 + BLOCK_COUNT) * MAX_SAVE_FILE_BYTES;

/// Whether stored save file `path` is the remembered sight's (spec 18, [`SIGHT_FILE`]; any case).
fn is_sight(path: &str) -> bool {
    path.eq_ignore_ascii_case(SIGHT_FILE)
}

/// The most bytes the page takes of stored save file `path`: [`MAX_SAVE_FILE_BYTES`], or for the
/// remembered sight's file (spec 18) the largest it can be,
/// `u6_core::gfx::cache::MAX_SIGHT_FILE_BYTES`.
fn save_file_limit(path: &str) -> usize {
    if is_sight(path) {
        MAX_SIGHT_FILE_BYTES
    } else {
        MAX_SAVE_FILE_BYTES
    }
}

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

/// [`save_file_limit`] of stored save file `path` (`savegame/<name>`) for the page, which leaves a
/// larger file of a dropped `savegame/` folder out before reading it ([`Game::load_save`] is the
/// backstop).
#[wasm_bindgen]
pub fn max_save_file_bytes_of(path: &str) -> f64 {
    save_file_limit(path) as f64
}

/// Whether stored save file `path` is the remembered sight's (spec 18), which [`MAX_SAVE_BYTES`]
/// does not count, for the page.
#[wasm_bindgen]
pub fn is_sight_file(path: &str) -> bool {
    is_sight(path)
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
/// Spec 17 Presentation, [`Game::hd_glyphs`]: the code of a cell drawn half-toned (a disabled
/// entry of the context menu) is this | its code ([`u6_render::hd::HdGlyph::halftone`]).
const HALFTONE: u32 = 0x4_0000;

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

/// Spec 19 O1c: the farthest a key event's `timeStamp` may lie from the page's clock
/// ([`Game::set_clock`]) and be taken as the press's time; past it the two are not one timeline
/// (a page that never set the clock, a host's own stamps) and the clock stands in.
const CLOCK_SKEW_MS: f64 = 1000.0;

/// The most characters one [`Game::text_input`] queues: more than a conversation's line holds
/// (the engine also keeps no more text queued than that, `u6_core::engine`'s text room).
const TEXT_CHARS: usize = 64;

/// Spec 19 O1b, O1c: one key held on the page: its DOM `code`, the engine key its presses and
/// repeats were queued as, its first event's time (`timeStamp`), and whether it walks: only a
/// key whose first event was its press, made with neither Ctrl nor Alt down ([`Game::key_down`]);
/// one first seen by a repeat ([`Game::key_repeat`]: its press was not the game's, a chord with
/// a modifier the page kept) never does, as in the native window.
#[derive(Clone, Debug, PartialEq)]
struct HeldKey {
    code: String,
    key: Key,
    at: f64,
    walking: bool,
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
    /// Spec 19 O1b: per physical key held (its DOM `code`), each engine key its presses and
    /// repeats were queued as, so its release takes back those ([`Game::key_up`]); spec 19 O1c:
    /// with its first event's time and whether it walks.
    held: Vec<HeldKey>,
    /// Spec 19 O1c: the page's clock at the step whose tick the command prompt last took a
    /// press in ([`u6_core::Engine::prompt_takes`]): the minimum interval runs from it.
    last_step: Option<f64>,
    /// Spec 19 O1c: the page's clock, the animation frame's time ([`Game::set_clock`]: the
    /// timeline of the key events' `timeStamp`), against which a held key's times are read.
    clock: f64,
    /// Spec 19 O1c: Ctrl or Alt down as of the last key event ([`Game::set_modifiers`]): a
    /// held walk pauses while one is.
    modifiers: bool,
    /// Spec 18: the remembered sight on ([`Game::set_sight_memory`]).
    sight_memory: bool,
    turn_speed: u6_core::turn_speed::TurnSpeed,
    /// Spec 17 Presentation: the frame rects the page shows something else over
    /// ([`Game::set_menu_hidden`]).
    menu_hidden: Vec<(i32, i32, i32, i32)>,
    /// Spec 17: the page's presses that reached no frame pixel while events waited for the next
    /// step ([`Game::dismiss_menu`]), each the index in `pending` it stands before.
    dismissals: Vec<usize>,
    control_action: bool,
    controls: Vec<(usize, bool)>,
    /// Spec 17: the page's clock as a whole millisecond once the page gave one
    /// ([`Game::set_clock`]), the context menu's double-click clock ([`Engine::set_wall_clock`]).
    wall: Option<u64>,
    /// Spec 20: smooth motion ([`Game::set_motion`]).
    motion: Motion<Shot>,
    /// Spec 20 Design 5: the slide's layers over the HD frame ([`Game::slide`]).
    layers: SlideImages,
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
            held: Vec::new(),
            last_step: None,
            clock: 0.0,
            modifiers: false,
            sight_memory: false,
            turn_speed: u6_core::turn_speed::TurnSpeed::Original,
            menu_hidden: Vec::new(),
            dismissals: Vec::new(),
            control_action: false,
            controls: Vec::new(),
            wall: None,
            motion: Motion::new(),
            layers: SlideImages::default(),
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

    /// The program that runs: `u`, `game`, `end`, `ended`, or empty before a start.
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
            u6_core::main_loop::Program::End => "end".into(),
        }
    }

    /// Spec 19 O1c: the presses the command prompt has taken in this game
    /// ([`u6_core::Engine::prompt_takes`]: a key, a paced press; never a repeat of a direction
    /// key, which the prompt takes back), 0 before a start. For the page's tests.
    pub fn prompt_takes(&self) -> f64 {
        self.engine
            .as_ref()
            .map_or(0.0, |e| e.prompt_takes() as f64)
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
    /// [`Game::start`], which lays them over the game files. A file over its limit
    /// ([`save_file_limit`]), or one that would take the files together past [`MAX_SAVE_BYTES`]
    /// (the remembered sight's file not counted), is left out (the start's report names it); a
    /// path given again replaces its file, and the remembered sight's file in any case replaces
    /// the one taken before. Returns whether the file was taken.
    pub fn load_save(&mut self, path: &str, bytes: Vec<u8>) -> bool {
        let sight = is_sight(path);
        let old = self.saves.get(path).map_or(0, Vec::len);
        let total = match sight {
            true => self.save_bytes,
            false => self.save_bytes - old + bytes.len(),
        };
        let most = save_file_limit(path);
        if bytes.len() > most || total > MAX_SAVE_BYTES {
            let limit = if bytes.len() > most {
                format!("a save file holds at most {most}")
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
        if sight {
            self.saves.retain(|p, _| !is_sight(p));
        }
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
    /// [`Game::save_path`] / [`Game::save_ptr`] / [`Game::save_len`]. The last list is let go
    /// first, so two lists never live together (spec 18, Bounds).
    pub fn save_collect(&mut self) -> u32 {
        let whole = self.save_whole;
        self.save_out = Vec::new();
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
    /// no key pending): the prompt is up. Not the context menu's gate (spec 17 round 4, opus N1:
    /// kept apart, the page's `data-prompt` and its tests' waits read this): that one is spec 19's
    /// command prompt ([`u6_core::engine::command_prompt`]: keys taken as commands, no
    /// cross-hair or panel cursor up), so with the more prompt up or a cursor up this is true
    /// while [`Game::context_menu`] gives `""` and a right press opens no menu.
    pub fn at_prompt(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| {
            e.state().main.reading == 1 && e.io().pending_events() == 0 && self.pending.is_empty()
        })
    }

    /// Spec 19 O1c receipts: a quiet command prompt where a held direction could be paced,
    /// excluding a more prompt or a target cursor. A keyup cooldown must reach this state,
    /// rather than merely the weaker [`Game::at_prompt`] read flag.
    pub fn command_prompt(&self) -> bool {
        self.at_prompt()
            && self
                .engine
                .as_ref()
                .is_some_and(|e| !e.finished() && u6_core::engine::command_prompt(&e.state()))
    }

    /// Spec 17: the context menu a right press at frame pixel (x, y) would open now: `""` unless
    /// the engine's gate lets a right press after the page's queued events open one
    /// ([`Engine::context_menu_gate`]: the command prompt up, nothing queued for the game but the
    /// repeats it takes back, the events queued since the last step inert) and the pixel is on
    /// the map or on an inventory or equipment slot (Wide's included); else JSON `{kind:
    /// "map"|"panel", cell: [x, y] | null, underlined: code | null, sections: [{object, kind:
    /// [type, frame] | null, entries: [{code, enabled, reason, theft}]}], go_here: {code,
    /// enabled, reason, theft} | null}`: codes only, no names (the engine draws
    /// the menu itself, `u6_core::ui::popup`). Reads the game only
    /// ([`u6_core::ui::menu::query`]); the page's tests use it.
    pub fn context_menu(&self, x: i32, y: i32) -> String {
        let Some(e) = self
            .engine
            .as_ref()
            .filter(|e| e.context_menu_gate(&self.pending))
        else {
            return String::new();
        };
        let word = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        u6_core::ui::menu::query(&e.state(), e.io(), word(x), word(y))
            .map_or_else(String::new, |m| menu_json(&m))
    }

    /// Spec 17 Presentation: the context menu the engine has open and drawn in the frame, `""`
    /// while none is: JSON `{rect: [x, y, w, h], text: [x, y, w, h], hover, korean, colours:
    /// [paper, ink, heading], rows: [{kind: "head"|"entry"|"note", code: code | null, enabled,
    /// reason: code | null, theft, y}]}`, the box with its frame and the text area in frame
    /// pixels, the highlighted row's index, whether it is laid out and drawn in the Korean words
    /// (the installed pack's language, `u6_core::locale::pack_is_korean`; no words), the colours
    /// its text area is drawn in (0xRRGGBB, the game's palette now: the page's tests compare
    /// pixels with them and report counts), each row's top pixel row; codes only
    /// ([`u6_core::Engine::context_menu`]).
    pub fn menu_view(&self) -> String {
        let Some(e) = self.engine.as_ref() else {
            return String::new();
        };
        let Some(v) = e.context_menu() else {
            return String::new();
        };
        let c = e.state().ui.tables.colours;
        let palette = &e.screen_frame().palette;
        let rgb = |i: u8| {
            let c = palette[usize::from(i)];
            u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
        };
        view_json(&v, [rgb(c.paper), rgb(c.text), rgb(c.alert)])
    }

    /// The Ctrl representative action as drawn, in the same code-only format as `menu_view`.
    pub fn quick_action_view(&self) -> String {
        let Some(e) = self.engine.as_ref() else {
            return String::new();
        };
        let Some(v) = e.quick_action() else {
            return String::new();
        };
        let c = e.state().ui.tables.colours;
        let palette = &e.screen_frame().palette;
        let rgb = |i: u8| {
            let c = palette[usize::from(i)];
            u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
        };
        view_json(&v, [rgb(c.paper), rgb(c.text), rgb(c.alert)])
    }

    /// Spec 17 Presentation: the frame rects the page shows something else over (in Wide its
    /// windows over the map and the map's parts off the canvas; the canvas's parts off the
    /// viewport and under the page's elements over it), as `rects` x, y, width, height in turn
    /// (frame pixels; a rect with no area or a trailing part is left out): the engine's context
    /// menu keeps off them where it can, one opened from then and one open now (laid out again at
    /// once, [`Engine::set_context_menu_hidden`]). Kept for every game started after
    /// ([`Game::menu_hidden`]).
    pub fn set_menu_hidden(&mut self, rects: Vec<i32>) {
        self.menu_hidden = rects
            .chunks_exact(4)
            .filter(|r| r[2] > 0 && r[3] > 0)
            .map(|r| (r[0], r[1], r[0] + r[2] - 1, r[1] + r[3] - 1))
            .collect();
        if let Some(engine) = &mut self.engine {
            engine.set_context_menu_hidden(self.menu_hidden.clone());
        }
    }

    /// Spec 17 Presentation: a press of the page's that reaches no frame pixel (off the canvas,
    /// in the HD layout's gaps, on the letterbox) closes an open context menu, as a press outside
    /// it in the frame does, in order with the events queued before it: while some wait for the
    /// next step it closes the menu there, after them ([`Engine::tick_dismissing`]; a right press
    /// queued just before opens the menu at that step, which it then closes), and returns false;
    /// else at once, true when one was open (the frame without it composed). Nothing reaches the
    /// game.
    pub fn dismiss_menu(&mut self) -> bool {
        let Some(engine) = &mut self.engine else {
            return false;
        };
        if self.pending.is_empty() {
            return engine.dismiss_context_menu();
        }
        self.dismissals.push(self.pending.len());
        false
    }

    /// Spec 17 Presentation: the frame rects the engine keeps its context menu off now
    /// ([`Engine::context_menu_hidden`], as the page last gave them, [`Game::set_menu_hidden`]),
    /// x, y, width, height in turn: what reached the engine, for the page's tests (the page's own
    /// `__u6Layout.hidden` is computed afresh); none without a game.
    pub fn menu_hidden(&self) -> Vec<i32> {
        self.engine.as_ref().map_or_else(Vec::new, |e| {
            e.context_menu_hidden()
                .iter()
                .flat_map(|&(x0, y0, x1, y1)| [x0, y0, x1 - x0 + 1, y1 - y0 + 1])
                .collect()
        })
    }

    /// Spec 17: how many times "Go here" asked travel ([`Engine::travel_to`], spec 21), counted
    /// from the start; 0 before one (counts only: no cell).
    pub fn go_here_asked(&self) -> u32 {
        self.engine
            .as_ref()
            .map_or(0, |e| e.go_here_asked().asked.len() as u32)
    }

    /// Spec 17 (round 2): the presses the engine let through to the game (button-down edges of
    /// the pointer events it queued for it, the context menu's own and those the message history
    /// takes left out, [`Engine::game_presses`]), counted from the start; 0 before one. With
    /// [`Game::game_buttons`] the page's tests see a press and its release within one step.
    pub fn game_presses(&self) -> f64 {
        self.engine
            .as_ref()
            .map_or(0.0, |e| e.game_presses() as f64)
    }

    /// Spec 17 (round 2): the key events (presses, repeats, releases) the engine let through to
    /// the game, the context menu's own left out, counted from the start; 0 before one.
    pub fn game_keys(&self) -> f64 {
        self.engine.as_ref().map_or(0.0, |e| e.game_keys() as f64)
    }

    /// Spec 17 (round 6): the lines the game's message scroll finished since the start (a count
    /// that only grows, `UiState::lines_finished`; a command that prints, as Look, adds to it;
    /// the lines the scroll keeps stop at 64); 0 before a start.
    pub fn lines_finished(&self) -> f64 {
        self.engine
            .as_ref()
            .map_or(0.0, |e| e.state().ui.lines_finished as f64)
    }

    /// The pointer's buttons as the game's input handler has them (bit 0 left, bit 1 right): the
    /// last pointer event the engine queued for the game ([`u6_core::Io::handler_pointer`]),
    /// which the context menu's own presses never reach; 0 before a start. The page's tests read
    /// what the game took with it.
    pub fn game_buttons(&self) -> u8 {
        self.engine
            .as_ref()
            .map_or(0, |e| e.io().handler_pointer().buttons & 3)
    }

    /// Port (spec 21): the travel of a right press's "move here": the party walks to map cell
    /// (x, y) on its level, one step a command prompt entered as its direction key, from the next
    /// [`Game::step`] on ([`u6_core::engine::Engine::travel_to`]). Any key or a button press after
    /// it stops the walk. False (nothing started) before a start or when the cell is refused
    /// ([`u6_core::actors::travel::reachable`]).
    pub fn travel_to(&mut self, x: u16, y: u16) -> bool {
        self.engine
            .as_mut()
            .is_some_and(|e| e.travel_to((x, y)).is_ok())
    }

    /// Port (spec 21): a travel walks ([`Game::travel_to`]).
    pub fn travel_active(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| e.travel_active())
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

    /// Whether the party is in combat mode (the combat byte, A4-0x7b0c) as of the last step; false
    /// before a start. Read by the tests' receipts of a step's time (spec 20 Design 1).
    pub fn combat_mode(&self) -> bool {
        self.engine
            .as_ref()
            .is_some_and(|e| e.state().actors.combat_mode())
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
        self.observe_motion(0.0);
        let due = self.held_due();
        if let Some(engine) = &mut self.engine {
            // Spec 19 O1c: a held direction key is pressed again at the command prompt, at most
            // once a `MIN_STEP_MS` on the page's clock from the press it last took.
            if let Some(i) = due
                && let Some(press) = engine.held_press(&self.pending, Some(self.held[i].key))
            {
                self.pending.push(press);
            }
            let takes = engine.prompt_takes();
            if let Some(wall) = self.wall {
                engine.set_wall_clock(wall);
            }
            // Spec 21 S3: a travel steps as a held arrow does, at most once a `MIN_STEP_MS` on the
            // page's clock from the press the prompt last took (its own key is one); until the
            // page gives a clock ([`Game::set_clock`]), at every prompt, as a headless run.
            let min = f64::from(u6_core::engine::MIN_STEP_MS);
            let paced = self.wall.is_some();
            engine.set_travel_due(!paced || self.last_step.is_none_or(|t| self.clock - t >= min));
            engine.tick_controls(&self.pending, &self.dismissals, &self.controls, ms);
            if engine.prompt_takes() != takes {
                self.last_step = Some(self.clock);
            }
            self.log.extend(engine.take_log());
            if let Some(driver) = &mut self.audio {
                driver.sync(&engine.state().sound, engine.io().files());
            }
            self.handed = last_buttons(&self.pending).unwrap_or(self.handed);
        }
        self.observe_motion(0.0);
        self.pending.clear();
        self.dismissals.clear();
        self.controls.clear();
    }

    /// Converts the engine's frame into the RGBA frame ([`Game::frame_ptr`]) when it changed
    /// since the last conversion (spec 06 O1, [`Engine::frame_version`]): once per display
    /// frame, after its steps; steps that composed no new frame convert nothing. Before a start
    /// the RGBA frame stays black.
    ///
    /// With HD on ([`Game::set_hd`]) it then composes the HD frame when the engine's screen frame
    /// changed since the last composition ([`Engine::screen_version`]: the published frame, the
    /// message history the player scrolled back to and the context menu over it; a move of the
    /// pointer sprite alone composes nothing, spec 11 Design 7), and takes the pointer the page
    /// draws over it ([`Game::hd_pointer`]).
    ///
    /// Spec 20: with smooth motion on, it hands the engine's publications to the presenter. While
    /// a slide runs with HD off it shows the slide's picture in the RGBA frame instead
    /// ([`Game::present_slide`]), and the present after the last one converts the engine's frame
    /// again. With HD on the HD frame stays the engine's, and the slide's layers, composed once a
    /// generation of the slides, are placed for the page to draw over its map ([`Game::slide`]).
    pub fn present(&mut self) {
        let view = self.slide_view();
        if !self.hd.on() && self.present_slide(view.as_ref()) {
            self.track_pointer();
            return;
        }
        let version = self.engine.as_ref().map(Engine::frame_version);
        if version.is_some() && version != self.rgba_holds {
            self.redraw();
        }
        if self.engine.is_some() {
            self.compose_hd();
        }
        self.place_slide(view.as_ref());
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

    /// Diagnostic whole-glyph cells of the last HD composition, five words per cell:
    /// x, y, code, ink RGB, paper RGB. Glyphs always retain their original pixel shapes.
    /// Codes are ASCII or Hangul, 0x20 for a whole space, [`TYPED`] | 0x20 for a typed
    /// space, [`SYMBOL`] | the original glyph for a paper symbol, and [`HALFTONE`] for
    /// half-toned cells. Empty while HD is off. This data never enables a font overlay.
    pub fn hd_glyphs(&self) -> Vec<u32> {
        let rgb = |c: u6_core::Rgb| u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b);
        self.hd
            .glyphs()
            .iter()
            .flat_map(|g| {
                [
                    u32::from(g.x),
                    u32::from(g.y),
                    listed_code(g.code) | if g.halftone { HALFTONE } else { 0 },
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
    /// engine placed its sprite at in the last step or since ([`Engine::pointer_at`]): the
    /// context menu's while it takes the pointer's events (spec 17: its arrow over the open menu),
    /// else the input handler's ([`u6_core::Io::handler_pointer`], the VBlank server's, which no
    /// other call moves); `shown` 1 while the sprite is drawn there ([`Engine::sprite`], the
    /// pointer not hidden: 0x1622d6 parks a hidden one below the frame), else 0, in any program
    /// and with HD on or off. `queued` 1 while a pointer event of the page's waits for the next
    /// step ([`Game::pointer`]), (qx, qy) the last one's pixel as queued; else 0, 0, 0. The queue
    /// is as of now: the presses of a context-menu entry chosen at the next step are the engine's
    /// own, given the game at that step after what was queued before, so (qx, qy) is where the
    /// sprite goes at the next step but for them, which the next read after that step shows. All
    /// 0 before a start.
    pub fn game_pointer(&self) -> Vec<i32> {
        let Some(e) = &self.engine else {
            return vec![0; 6];
        };
        let at = e.pointer_at();
        let shown = e.sprite().is_some() && e.state().ui.pointer.shown;
        let queued = self.pending.iter().rev().find_map(|ev| match *ev {
            InputEvent::Pointer { x, y, .. } => Some((x, y)),
            InputEvent::Key(_)
            | InputEvent::Repeat(_)
            | InputEvent::Paced(_)
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
        self.motion.frame(dt);
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

    /// Spec 19 O3: 1 original (default), 2 quick, 4 fast, 0 instant.
    /// Only turn presentation waits change; the clock, music and opening keep their pace.
    pub fn set_turn_speed(&mut self, factor: u8) {
        self.turn_speed = u6_core::turn_speed::TurnSpeed::from_code(factor);
        if let Some(engine) = &self.engine {
            engine.io().set_turn_speed(self.turn_speed);
        }
    }

    pub fn turn_speed(&self) -> u8 {
        self.turn_speed.code()
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
    /// false), at `at` (its `timeStamp`, the timeline of [`Game::set_clock`]). Returns true when
    /// it is a game key (the caller should then `preventDefault` so arrows do not scroll the
    /// page and Ctrl-S / Ctrl-R do not save or reload the page).
    pub fn key_down(&mut self, code: &str, key: &str, ctrl: bool, at: f64) -> bool {
        self.key_event(InputEvent::Key, code, key, ctrl, at)
    }

    /// Spec 19 O1b: queues a press the browser's auto-repeat sent (a `keydown` whose `repeat`
    /// is true), as [`Game::key_down`] does a press; the key's release ([`Game::key_up`]) takes
    /// it back while it waits at a key wait that drains.
    pub fn key_repeat(&mut self, code: &str, key: &str, ctrl: bool, at: f64) -> bool {
        self.key_event(InputEvent::Repeat, code, key, ctrl, at)
    }

    /// Spec 19 O1b: the physical key `code` (a `keyup`'s) let go: one release per engine key
    /// its presses and repeats were queued as ([`Game::key_down`], [`Game::key_repeat`]),
    /// whatever the modifiers are now (Ctrl let go before S still releases Ctrl-S). Outside the
    /// line editor a release takes the key's queued repeats back, so a held key walks no
    /// further than the prompt it was let go at. Whether `code` was held.
    pub fn key_up(&mut self, code: &str) -> bool {
        let (pending, running) = (&mut self.pending, self.engine.is_some());
        let before = self.held.len();
        self.held.retain(|h| {
            if h.code != code {
                return true;
            }
            if running {
                pending.push(InputEvent::Release(h.key));
            }
            false
        });
        self.held.len() != before
    }

    /// Spec 19 O1b: every held key let go ([`Game::key_up`]), for when the page can no longer
    /// see their releases (the window lost the focus or the page was hidden).
    pub fn release_keys(&mut self) {
        self.motion.attacks.clear();
        if let Some(engine) = &self.engine {
            self.motion.attack_barrier = Some(engine.io().now_ms() as f64);
            engine.io().take_attacks();
        }
        let codes: Vec<String> = self.held.iter().map(|h| h.code.clone()).collect();
        for code in codes {
            self.key_up(&code);
        }
        self.modifiers = false;
        self.set_control_action(false);
    }

    /// Spec 19 O1c: the page's clock now, the animation frame's time (on the timeline of the
    /// key events' `timeStamp`, which [`Game::key_down`] takes), read by the held walk's interval
    /// ([`u6_core::engine::paced_press_due`]). Called once a frame, before its steps.
    pub fn set_clock(&mut self, now: f64) {
        if now.is_finite() {
            self.clock = now;
            // Spec 17: the context menu's double click is timed on the page's clock too.
            self.wall = Some(now.max(0.0) as u64);
        }
    }

    /// Spec 19 O1c: whether Ctrl or Alt is down, as the last key event reports it: while one
    /// is, a held direction key adds no press (the walk pauses, as in the native window) and
    /// nothing is let go.
    pub fn set_modifiers(&mut self, down: bool) {
        self.modifiers = down;
    }

    /// Ctrl alone selects the representative action. Ordered alongside queued pointer events,
    /// so releasing Ctrl in the click's frame cannot change which command that click chose.
    pub fn set_control_action(&mut self, down: bool) {
        if self.control_action != down {
            self.control_action = down;
            if self.engine.is_some() {
                self.controls.push((self.pending.len(), down));
            }
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
            // A move after a queued dismissal stays after it.
            let kept = self
                .dismissals
                .last()
                .copied()
                .unwrap_or(0)
                .max(self.controls.last().map_or(0, |&(i, _)| i));
            queue_pointer(&mut self.pending, kept, self.handed, x, y, buttons);
        }
    }

    /// Spec 23: the wheel at frame pixel (x, y) (as [`Game::pointer`] maps it), `dy` lines (< 0
    /// older, > 0 newer): over the message window while `game` runs it scrolls the message
    /// history the frame shows, at once ([`Game::present`] shows it), and returns true (the caller
    /// should then `preventDefault`); elsewhere, or before a start, false and nothing changes.
    /// `dy` 0 moves nothing: whether the wheel is taken there (the page asks before it adds a
    /// pixel delta to its carry). Never the game's input.
    pub fn wheel(&mut self, x: i32, y: i32, dy: i32) -> bool {
        let word = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        self.engine
            .as_mut()
            .is_some_and(|e| e.wheel(word(x), word(y), dy))
    }

    /// Spec 23: lines the message history is scrolled back from the live bottom (0: live, and
    /// before a start).
    pub fn history_offset(&self) -> u32 {
        self.engine
            .as_ref()
            .map_or(0, |e| u32::try_from(e.history_offset()).unwrap_or(u32::MAX))
    }

    /// Spec 22 Contract 8: whether the game reads the player's words by click now (a
    /// conversation's read after "you say", op f7, from its start to its end; buttons drawn or
    /// not). The page releases a cancelled press off every target then. False before a start.
    pub fn reading_by_click(&self) -> bool {
        self.engine
            .as_ref()
            .is_some_and(|e| e.state().ui.talk.reading)
    }

    /// Spec 22 Contract 6, for the page's tests: [hot, pressed] of the read by click, the
    /// target drawn inverse and the one the left button went down on, as the engine holds them
    /// after its last step: each -1 for none, `i` for the button `i` (0 Job, 1 Name), 0x10000 +
    /// row × 0x100 + col for the keyword whose first part starts at message-window cell (row,
    /// col); 0x20000 + next for a page arrow, 0x30000 + yes for a restricted answer.
    /// [-1, -1] before a start and outside a read.
    pub fn talk_target(&self) -> Vec<i32> {
        let code = |t: Option<u6_core::ui::talk::Target>| match t {
            None => -1,
            Some(u6_core::ui::talk::Target::Button(i)) => i as i32,
            Some(u6_core::ui::talk::Target::Page(next)) => 0x20000 + i32::from(next),
            Some(u6_core::ui::talk::Target::Answer(yes)) => 0x30000 + i32::from(yes),
            Some(u6_core::ui::talk::Target::Key { row, col }) => {
                0x10000 + i32::from(row) * 0x100 + i32::from(col)
            }
        };
        self.engine.as_ref().map_or_else(
            || vec![-1, -1],
            |e| {
                let talk = &e.state().ui.talk;
                vec![code(talk.hot), code(talk.pressed)]
            },
        )
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

    /// Spec 20: smooth motion on or off: while on, the leader's step and every object that moved
    /// one cell slide there on the presentation clock ([`Game::present`]), with base tap timing
    /// or the held party mover's own measured cadence; off
    /// (the default), the frames are the engine's. Applies to the game running (from its next
    /// picture of the map) and to every game started after.
    pub fn set_motion(&mut self, on: bool) {
        self.motion.on = on;
        self.motion.clear();
        if let Some(engine) = &mut self.engine {
            engine.set_motion(on);
        }
    }

    /// Display-only walking pose: 0 Off, 1 one pixel, 2 two pixels (default), 3 three pixels.
    pub fn set_footstep_pattern(&mut self, code: u8) {
        self.motion.feet.set_pattern(Pattern::from_code(code));
        self.motion.poses.clear();
        self.motion.shown = None;
        self.layers.made_for = None;
        self.rgba_holds = None;
    }

    pub fn footstep_pattern(&self) -> u8 {
        self.motion.feet.pattern().code()
    }

    /// Last displayed human poses as [actor slot, frame phase, upward original-pixel offset].
    pub fn footstep_poses(&self) -> Vec<i32> {
        self.motion
            .poses
            .iter()
            .flat_map(|p| [i32::from(p.slot), i32::from(p.phase), p.lift])
            .collect()
    }

    /// Display-only impulse receipt: actor slot, horizontal offset, vertical offset.
    pub fn attack_offsets(&self) -> Vec<i32> {
        self.motion
            .poses
            .iter()
            .filter(|p| p.offset != (0, 0))
            .flat_map(|p| [i32::from(p.slot), p.offset.0, p.offset.1])
            .collect()
    }

    /// The page's chooser owns the display while open; simulation continues unchanged.
    pub fn set_attack_suspended(&mut self, suspended: bool) {
        self.motion.attack_suspended = suspended;
        if suspended {
            self.motion.attacks.clear();
            if let Some(e) = &self.engine {
                e.io().take_attacks();
            }
        }
    }

    /// Whether smooth motion is on ([`Game::set_motion`]).
    pub fn motion(&self) -> bool {
        self.motion.on
    }

    /// Spec 20: the presentation clock: the engine's milliseconds and the steps' time the wall
    /// clock is ahead of them at the speed ([`u6_core::pacer::Pacer::carried_ms`]), never before
    /// the last displayed frame even when changing speed clears that carry; 0 before a start.
    pub fn presentation_ms(&self) -> f64 {
        self.engine.as_ref().map_or(0.0, |e| {
            (e.io().now_ms() as f64 + self.pacer.carried_ms()).max(self.motion.presented_ms)
        })
    }

    /// Spec 20: whether the next present draws a slide, or ends one (the page presents then with
    /// no step too).
    pub fn sliding(&self) -> bool {
        self.motion.sliding
            || self.motion.presenter.active(self.presentation_ms())
            || self.motion.attacks.active(self.presentation_ms())
    }

    /// Spec 20 Design 3 (round 13, the receipts): a foreign picture came after the leader's
    /// early picture with no scene of the engine's since, so the engine's frame, where the page
    /// shows it, holds the map as the engine published it before the step: the party on its old
    /// cell.
    pub fn old_cell(&self) -> bool {
        self.motion.old_cell
    }

    /// Spec 20 Design 5: moves whenever what the page draws of a slide over the HD frame's map
    /// changed ([`Game::slide`]): its layers, where they are, or the end of a slide (the page puts
    /// the HD frame's map back).
    pub fn slide_version(&self) -> f64 {
        self.layers.version as f64
    }

    /// Spec 20 Design 5: with HD on, the slide the page draws over the HD frame's map now, clipped
    /// to the shown map ([`shown_map`]), as (layer, x, y) triples in draw order, (x, y) the frame
    /// pixel of the layer's top left ([`Game::slide_layer_size`]); empty when no slide runs (the HD
    /// frame shows the map).
    pub fn slide(&self) -> Vec<i32> {
        self.layers.placed.clone()
    }

    /// Centre of the newest presented map scene. It can trail the authoritative leader
    /// while an unsafe preview is withheld; receipts must not confuse the two timelines.
    pub fn slide_centre(&self) -> Vec<i32> {
        self.motion
            .presenter
            .history()
            .first()
            .map_or_else(Vec::new, |s| vec![s.scene.centre.0, s.scene.centre.1])
    }

    /// Timing metadata for the currently running presentation tracks, in groups of five:
    /// slot (-1 camera), start, duration, initial x/y offset. Used by frame receipts.
    pub fn slide_timings(&self) -> Vec<f64> {
        self.motion
            .presenter
            .timings()
            .into_iter()
            .flat_map(|(s, t, d, x, y)| [f64::from(s), t, d, x, y])
            .collect()
    }

    /// Track slot and actual path distance; automatic routes include every pending corner.
    pub fn slide_path_distances(&self) -> Vec<f64> {
        self.motion
            .presenter
            .path_distances()
            .into_iter()
            .flat_map(|(slot, distance)| [f64::from(slot), distance])
            .collect()
    }

    /// Current retained scenes, static source pixels and resource resets since motion clear.
    /// The browser receipts distinguish a resource fallback from a continuous track.
    pub fn slide_resources(&self) -> Vec<f64> {
        let presenter = &self.motion.presenter;
        vec![
            presenter.history().len() as f64,
            presenter.static_bounds().map_or(0, |(_, _, w, h)| w * h) as f64,
            presenter.capacity_resets() as f64,
        ]
    }

    /// Layer i's pixel origin at rest. Its placement minus this origin is its drawn offset;
    /// W3's static layer can extend farther than one cell when a track is retargeted.
    pub fn slide_layer_origin(&self, i: u32) -> Vec<i32> {
        self.layers
            .slide
            .as_ref()
            .and_then(|s| s.parts.get(i as usize))
            .map_or_else(Vec::new, |p| vec![p.pixels.x, p.pixels.y])
    }

    /// Spec 20 Design 5: moves whenever the slide's layers are made again (their count, sizes and
    /// pixels).
    pub fn slide_layers_version(&self) -> f64 {
        self.layers.made as f64
    }

    /// Spec 20 Design 5: the slide's layers made last.
    pub fn slide_layer_count(&self) -> u32 {
        self.layers.images.len() as u32
    }

    /// Spec 20 Design 5: the layers version ([`Game::slide_layers_version`]) of the static and
    /// frame layers the changes of the layers made last apply to ([`Game::slide_layer_patch`]):
    /// the last the page said it holds ([`Game::slide_applied`]); -1 for none.
    pub fn slide_layers_base(&self) -> f64 {
        self.layers.held.as_ref().map_or(-1.0, |k| k.made as f64)
    }

    /// Spec 20 Design 5: the page holds the layers of version `version` (its static and frame
    /// layers are what the next changes apply to).
    pub fn slide_applied(&mut self, version: f64) {
        if self
            .layers
            .pending
            .as_ref()
            .is_some_and(|k| k.made as f64 == version)
        {
            self.layers.held = self.layers.pending.take();
        }
    }

    /// Spec 20 Design 5: the page does not hold the layers the changes apply to (a new canvas):
    /// the next present makes every layer whole, and no report of layers made before counts.
    pub fn slide_reset(&mut self) {
        self.layers.held = None;
        self.layers.pending = None;
        self.layers.made_for = None;
    }

    /// Spec 20 Design 5: layer `i` as changes from the one of the same place the page holds
    /// ([`Game::slide_layers_base`]): its pixels moved by (x, y), then `n` rects (x, y, width,
    /// height in frame pixels from its top left) whose pixels follow ([`Game::slide_patch_ptr`]):
    /// `[x, y, n, rects…]`; empty when the layer is given whole ([`Game::slide_layer_ptr`]).
    pub fn slide_layer_patch(&self, i: u32) -> Vec<i32> {
        let Some(p) = self
            .layers
            .images
            .get(i as usize)
            .and_then(|l| l.patch.as_ref())
        else {
            return Vec::new();
        };
        let mut out = vec![p.shift.0, p.shift.1, p.rects.len() as i32];
        out.extend(p.rects.iter().flat_map(|(r, _)| r.map(|v| v as i32)));
        out
    }

    /// Spec 20 Design 5: the address of rect `k`'s RGBA8 pixels of layer `i`'s changes (at the HD
    /// scale; views over it are invalidated when memory grows).
    pub fn slide_patch_ptr(&self, i: u32, k: u32) -> *const u8 {
        self.patch_words(i, k)
            .map_or(std::ptr::null(), |w| w.as_ptr().cast())
    }

    /// Spec 20 Design 5: the length in bytes of rect `k`'s pixels of layer `i`'s changes.
    pub fn slide_patch_len(&self, i: u32, k: u32) -> usize {
        self.patch_words(i, k).map_or(0, |w| w.len() * 4)
    }

    /// Spec 20 Design 5: layer `i`'s size in frame pixels (width, height); its pixels are that
    /// times the HD scale ([`Game::hd_scale`]) each way. Empty past the last layer.
    pub fn slide_layer_size(&self, i: u32) -> Vec<u32> {
        self.layers
            .images
            .get(i as usize)
            .map_or_else(Vec::new, |l| vec![l.width as u32, l.height as u32])
    }

    /// Spec 20 Design 5: what layer `i` of the slide's layers shows
    /// ([`u6_core::gfx::motion::Role`]): 0 the static words (drawn at the camera's offset), 1 the
    /// cursor and the border (at rest), 2 a mover's own words, 3 the words over a mover; -1 past
    /// the last layer.
    pub fn slide_layer_role(&self, i: u32) -> i32 {
        use u6_core::gfx::motion::Role;
        let part = (self.layers.slide.as_ref()).and_then(|s| s.parts.get(i as usize));
        part.map_or(-1, |p| match p.role {
            Role::Static => 0,
            Role::Frame => 1,
            Role::Mover(_) => 2,
            Role::Over(_) => 3,
        })
    }

    /// Original actor/object slot of a mover or occluder layer; -1 for static/frame/invalid.
    pub fn slide_layer_slot(&self, i: u32) -> i32 {
        use u6_core::gfx::motion::Role;
        let Some(slide) = &self.layers.slide else {
            return -1;
        };
        match slide.parts.get(i as usize).map(|p| p.role) {
            Some(Role::Mover(k) | Role::Over(k)) => {
                slide.layers.movers.get(k).map_or(-1, |m| i32::from(m.slot))
            }
            _ => -1,
        }
    }

    /// Spec 20 Design 5: the address of layer `i`'s RGBA8 pixels in wasm memory (transparent where
    /// it draws nothing; views over it are invalidated when memory grows); none for a layer given
    /// as changes ([`Game::slide_layer_patch`]).
    pub fn slide_layer_ptr(&self, i: u32) -> *const u8 {
        self.layers
            .images
            .get(i as usize)
            .map_or(std::ptr::null(), |l| l.rgba.as_ptr().cast())
    }

    /// Spec 20 Design 5: the length in bytes of layer `i`'s pixels.
    pub fn slide_layer_len(&self, i: u32) -> usize {
        self.layers
            .images
            .get(i as usize)
            .map_or(0, |l| l.rgba.len() * 4)
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

    /// Spec 17 (round 2, tests): the engine's published frame (the screen as the game drew it:
    /// no context menu, no pointer sprite) as RGBA8, `width * height * 4` bytes, a copy; empty
    /// before a start. With [`Game::frame_rgba`] a test tells where the game's own screen changed
    /// from what the menu left behind.
    pub fn published_rgba(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(e) = &self.engine {
            e.published_frame().write_rgba(&mut out);
        }
        out
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
    /// runs, noting it as held by `code` from `at` (the event's `timeStamp`; the page's clock
    /// instead when the two are more than [`CLOCK_SKEW_MS`] apart, a time on another timeline)
    /// if it is not held yet, walking when `event` is the press and no modifier is down; whether
    /// it is a game key.
    fn key_event(
        &mut self,
        event: fn(Key) -> InputEvent,
        code: &str,
        key: &str,
        ctrl: bool,
        at: f64,
    ) -> bool {
        match key_from_dom(code, key, ctrl) {
            Some(k) => {
                if self.engine.is_some() {
                    self.pending.push(event(k));
                    if !self.held.iter().any(|h| h.code == code && h.key == k) {
                        let near = (at - self.clock).abs() <= CLOCK_SKEW_MS;
                        self.held.push(HeldKey {
                            code: code.to_string(),
                            key: k,
                            at: if near { at } else { self.clock },
                            walking: event(k) == InputEvent::Key(k) && !self.modifiers,
                        });
                    }
                }
                true
            }
            None => false,
        }
    }

    /// Spec 19 O1c: the held key (its index in `held`) the next step may press again: the
    /// newest walking direction key ([`u6_core::engine::newest_direction`]; one pressed with
    /// Ctrl is no game key, [`key_from_dom`], one with Alt or Meta never reaches the game, and
    /// one first seen by a repeat does not walk) when its paced press is due on the page's clock
    /// ([`u6_core::engine::paced_press_due`], from the last press the prompt took) and neither
    /// Ctrl nor Alt is down now.
    fn held_due(&self) -> Option<usize> {
        if self.modifiers {
            return None;
        }
        let walking = self.held.iter().enumerate().filter(|(_, h)| h.walking);
        let (_, i) = u6_core::engine::newest_direction(walking.map(|(i, h)| (h.key, i)))?;
        let (h, now) = (&self.held[i], self.clock);
        u6_core::engine::paced_press_due(now - h.at, self.last_step.map(|t| now - t)).then_some(i)
    }

    /// A pointer button is down as the input handler has it: the last pointer event queued
    /// since the previous step (newer than the engine's), else the engine's handler pointer
    /// ([`u6_core::Io::handler_pointer`]), or a press the message history holds (spec 23: the
    /// handler sees no button then, [`u6_core::Engine::history_holding`]) unless the last pointer
    /// event queued has every button up (the step that takes it ends the press: paced as
    /// released, as a game press is). False before a start.
    fn pointer_held(&self) -> bool {
        let queued = last_buttons(&self.pending);
        let buttons = queued.or_else(|| {
            self.engine
                .as_ref()
                .map(|e| e.io().handler_pointer().buttons)
        });
        let holding = queued.is_none_or(|b| b & 3 != 0)
            && self.engine.as_ref().is_some_and(Engine::history_holding);
        buttons.is_some_and(|b| b & 3 != 0) || holding
    }

    /// `engine`, just started, becomes the game on screen: no Save of it taken yet, no sound
    /// machine, no queued input, the voices and the remembered sight as set; its first frame
    /// converted and, with HD on, composed whole (its versions start again at 0).
    fn put_on_screen(&mut self, mut engine: Engine) {
        engine.set_speech(self.voices);
        // Spec 17: the page's right press opens the engine's context menu.
        engine.set_context_menu(true);
        engine.set_context_menu_hidden(self.menu_hidden.clone());
        engine.io().set_sight_memory(self.sight_memory);
        engine.io().set_turn_speed(self.turn_speed);
        engine.set_motion(self.motion.on);
        self.motion.clear();
        self.slide_reset();
        self.motion.drawn = false;
        self.motion.shown = None;
        self.saved_gen = 0;
        self.save_out = Vec::new();
        self.save_whole = false;
        self.save_out_whole = false;
        self.audio = None;
        self.engine = Some(engine);
        self.pending.clear();
        self.dismissals.clear();
        self.controls.clear();
        self.control_action = false;
        self.handed = 0;
        self.held.clear();
        self.last_step = None;
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

    /// Spec 20: hands the presenter the engine's publications since the last present; the slides
    /// as drawn at the presentation clock while one runs. A scene that snaps drops the layers the
    /// page holds as the base of the next changes: the next layers come whole.
    ///
    /// Spec 20 Design 3 (round 13): with the engine's command prompt up
    /// ([`u6_core::engine::command_prompt`]) a held early picture ends ([`Presenter::prompt`]).
    ///
    /// Read-only authoritative cell observations. They do not depend on a scene being
    /// published or on W2 accepting an early preview.
    fn observe_motion(&mut self, carried_ms: f64) {
        if !self.motion.on {
            return;
        }
        let direction = (!self.modifiers)
            .then(|| {
                u6_core::engine::newest_direction(
                    self.held.iter().filter(|h| h.walking).map(|h| (h.key, ())),
                )
            })
            .flatten()
            .map(|(key, ())| match key {
                Key::Up => (0, -1),
                Key::Down => (0, 1),
                Key::Left => (-1, 0),
                Key::Right => (1, 0),
                _ => unreachable!("newest_direction only returns cardinal keys"),
            });
        let Some(engine) = &self.engine else {
            return;
        };
        let route = direction
            .is_none()
            .then(|| engine.travel_session())
            .flatten();
        let direction = direction.or_else(|| engine.continuous_walk_direction());
        let time = engine.io().now_ms() as f64;
        let s = engine.state();
        let p = &s.actors.party;
        let party: Vec<u16> = if direction.is_some() && !engine.context_menu_open() {
            p.members[..p.count()]
                .iter()
                .map(|&n| u16::from(n))
                .collect()
        } else {
            Vec::new()
        };
        #[cfg(test)]
        let party = if self.motion.cadence {
            party
        } else {
            Vec::new()
        };
        let transition = (time + carried_ms).max(self.motion.presented_ms);
        if let Some(session) = route {
            self.motion
                .presenter
                .set_route_walking(&party, session, transition);
        } else {
            self.motion
                .presenter
                .set_walking(&party, direction.unwrap_or((0, 0)), transition);
        }
        self.motion.presenter.observe(
            -1,
            (i32::from(s.map.centre.x), i32::from(s.map.centre.y)),
            time,
        );
        for slot in party {
            let at = s.objects.location[usize::from(slot)];
            self.motion.presenter.observe(
                i32::from(slot),
                (i32::from(at.x), i32::from(at.y)),
                time,
            );
        }
    }

    /// Spec 20 with spec 17: while a context menu is open ([`Engine::context_menu_open`]) no slide
    /// runs. One running when the menu opens, or started under it, ends at once
    /// ([`Presenter::snap`]: not a publication; the presenter keeps the scene it shows), so the
    /// page shows the engine's frame with the menu: the menu is never drawn over a moving map,
    /// nor a moving map under it, and nothing slides on once it closes.
    fn slide_view(&mut self) -> Option<View> {
        self.observe_motion(self.pacer.carried_ms());
        let engine = self.engine.as_mut()?;
        let t = engine.io().now_ms() as f64 + self.pacer.carried_ms();
        let mut view = self.motion.take(engine, self.pacer.pace_rate(), t);
        if engine.context_menu_open() && view.take().is_some() {
            self.motion.presenter.snap();
        }
        // After a scene that snaps no change is made against layers made before it: after a load
        // the remembered sight's copies are forgotten and their sources named again by others. A
        // dissolve's or an XOR plot's snap drops them too, so the next layers come whole.
        if std::mem::take(&mut self.motion.snapped) {
            self.layers.held = None;
            self.layers.pending = None;
            self.motion.feet.clear();
        }
        self.motion.sliding = view.is_some();
        let eligible = self.motion.on
            && self.motion.world
            && (!self.motion.previewing || view.is_some())
            && !engine.context_menu_open()
            && engine.state().main.program == u6_core::main_loop::Program::Game;
        self.motion.poses = if eligible {
            u6_core::gfx::motion::footstep_poses(
                &self.motion.presenter,
                &mut self.motion.feet,
                view.as_ref(),
            )
        } else {
            self.motion.feet.clear();
            Vec::new()
        };
        if eligible {
            self.motion
                .attacks
                .apply(self.motion.presented_ms, &mut self.motion.poses);
        } else {
            self.motion.attacks.clear();
        }
        self.motion.sliding |= self.motion.attacks.active(self.motion.presented_ms);
        view.or_else(|| {
            (!self.motion.poses.is_empty()).then(|| View {
                camera: (0, 0),
                movers: Vec::new(),
            })
        })
    }

    /// Spec 20, HD off: with a slide (`view`), puts the engine's shown frame ([`Engine::frame`]:
    /// the screen frame, the published frame with the message history the player scrolled back
    /// to over it, spec 23, and an open context menu over both, spec 17; then the pointer sprite)
    /// with the map as the slides have it ([`u6_core::gfx::motion::paint`]) and the pointer sprite
    /// over it again into the RGBA frame (nothing when it holds that picture already: a preview
    /// held at rest). The map painted here would cover a menu over it: that holds only because no
    /// slide runs with the menu open ([`Game::slide_view`]'s snap). Returns whether it did; the
    /// first present after the last slide converts the engine's frame again.
    fn present_slide(&mut self, view: Option<&View>) -> bool {
        let Some(engine) = &self.engine else {
            return false;
        };
        let Some(view) = view else {
            if std::mem::take(&mut self.motion.drawn) {
                self.rgba_holds = None;
                self.motion.shown = None;
            }
            return false;
        };
        let newest = self.motion.presenter.history().first().map(Rc::as_ptr);
        let shown = (
            view.clone(),
            newest,
            engine.frame_version(),
            self.motion.poses.clone(),
        );
        if self.motion.drawn && self.motion.shown.as_ref() == Some(&shown) {
            return true;
        }
        self.motion.shown = Some(shown);
        let fb = &mut self.motion.frame;
        fb.clone_from(engine.frame());
        u6_core::gfx::motion::paint_footsteps(
            &engine.state(),
            &self.motion.presenter,
            view,
            &self.motion.poses,
            fb,
        );
        if let Some(sprite) = engine.sprite() {
            sprite.draw(fb);
        }
        fb.write_rgba(&mut self.rgba);
        self.rgba_holds = None;
        self.conversions += 1;
        self.motion.drawn = true;
        true
    }

    /// The words of rect `k` of layer `i`'s changes ([`Game::slide_layer_patch`]).
    fn patch_words(&self, i: u32, k: u32) -> Option<&[u32]> {
        let layer = self.layers.images.get(i as usize)?;
        let (_, words) = layer.patch.as_ref()?.rects.get(k as usize)?;
        Some(words)
    }

    /// Spec 20 Design 5, HD on: with a slide (`view`), the slide's layers made for the presenter's
    /// generation and the HD scale ([`u6_core::gfx::motion::slide_layers`], each composed once,
    /// [`HdView::compose_layer`]), and where the page draws them now
    /// ([`u6_core::gfx::motion::slide_placements`]); without one none placed. A slide drawn in the
    /// RGBA frame before HD came on is the engine's frame again at the next conversion.
    fn place_slide(&mut self, view: Option<&View>) {
        if std::mem::take(&mut self.motion.drawn) {
            self.rgba_holds = None;
            self.motion.shown = None;
        }
        let placed = match (&self.engine, view) {
            (Some(engine), Some(view)) if self.hd.on() => {
                let made_for = LayerKey {
                    generation: self.motion.presenter.generation(),
                    hd: self.hd.version(),
                    poses: self.motion.poses.clone(),
                    source: self
                        .motion
                        .presenter
                        .history()
                        .first()
                        // Unposed slides keep their generation's pixels until they settle.
                        // Posed/rest layers also need the newest shot, including idle scenery.
                        .filter(|_| !self.motion.poses.is_empty())
                        .map(|shot| ShotIdentity(Rc::downgrade(shot))),
                    movers: view.movers.len(),
                    palette: engine.published_frame().palette,
                };
                if self.layers.made_for.as_ref() != Some(&made_for) {
                    self.layers.make(
                        engine,
                        &self.motion.presenter,
                        &self.motion.poses,
                        &mut self.hd,
                    );
                    self.layers.made_for = Some(made_for);
                    self.layers.version += 1;
                }
                self.layers.placements(view)
            }
            _ => Vec::new(),
        };
        if placed != self.layers.placed {
            self.layers.placed = placed;
            self.layers.version += 1;
        }
    }

    /// With HD on: composes the HD frame from the engine's screen frame ([`Engine::screen_frame`]:
    /// the published frame with the history view and the menu over it; black before a start) when
    /// it does not hold that frame's version at the HD setting now ([`HdView::present`]).
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

/// Spec 20: what a present drew of a slide in the RGBA frame: the slides, the newest scene and the
/// engine frame's version.
type Drawn<T> = (View, Option<*const T>, u64, Vec<Pose>);

#[derive(PartialEq)]
struct LayerKey {
    generation: u64,
    hd: u64,
    poses: Vec<Pose>,
    source: Option<ShotIdentity>,
    movers: usize,
    palette: [u6_render::Rgb; 256],
}

/// Retain the allocation identity without retaining an old shot's scene buffers. A weak
/// reference prevents address reuse until the key is replaced, unlike a raw Rc pointer.
/// Engine-only publications (for example a fade over a held slide) leave this key unchanged.
struct ShotIdentity(std::rc::Weak<Shot>);

impl PartialEq for ShotIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

/// Spec 20 Design 5: the slide's layers the page draws over the HD frame's map.
#[derive(Default)]
struct SlideImages {
    /// What they were made for: the presenter's generation and the HD view's version (its scale,
    /// pack and switch).
    made_for: Option<LayerKey>,
    slide: Option<u6_core::gfx::motion::Slide>,
    images: Vec<LayerImage>,
    /// Bumped each time they are made ([`Game::slide_layers_version`]).
    made: u64,
    /// The static and frame layers as the page holds them ([`Game::slide_applied`]), and those of
    /// the layers made last, until the page says it holds them.
    held: Option<Kept>,
    pending: Option<Kept>,
    /// Where they are drawn now ([`Game::slide`]) and its version ([`Game::slide_version`]).
    placed: Vec<i32>,
    version: u64,
}

/// The static and frame layers' pixels of the layers made at `made`, composed under HD view
/// version `hd` (its scale, pack and switch) in `palette`, the static ones round the view centred
/// on world cell `centre`: changes from them are changes of colour index and source only while
/// all of those stay.
struct Kept {
    made: u64,
    hd: u64,
    palette: [u6_render::Rgb; 256],
    centre: (i32, i32),
    statics: Rc<u6_render::motion::LayerPixels>,
    frame: Rc<u6_render::motion::LayerPixels>,
}

/// One composed layer: its size in frame pixels and its packed RGBA words at the HD scale, or the
/// changes from the one the page holds ([`Patch`]).
struct LayerImage {
    width: usize,
    height: usize,
    rgba: Vec<u32>,
    patch: Option<Patch>,
}

/// The changes of a static or frame layer from the one the page holds: its pixels moved by
/// `shift` (frame pixels), then these rects (x, y, width, height in frame pixels from its top
/// left) with their words.
struct Patch {
    shift: (i32, i32),
    rects: Vec<([usize; 4], Vec<u32>)>,
}

impl SlideImages {
    /// Rasterizes the slide's layers of `presenter` from `engine`'s state and composes them in
    /// `hd`: the static layer and the frame layer as their changes from those the page holds
    /// (the static one moved with the camera from the view the page's was made round; whole when
    /// it holds none made under this HD view version and palette), the others whole.
    fn make(
        &mut self,
        engine: &Engine,
        presenter: &Presenter<Shot>,
        poses: &[Pose],
        hd: &mut HdView,
    ) {
        use u6_core::gfx::motion::Role;
        let slide = u6_core::gfx::motion::slide_layers_footsteps(&engine.state(), presenter, poses);
        let parts = slide.as_ref().map_or(&[][..], |s| &s.parts[..]);
        let version = hd.version();
        let palette = engine.published_frame().palette;
        let centre = presenter
            .history()
            .first()
            .map_or((0, 0), |s| (s.scene.centre.0, s.scene.centre.1));
        let held = self
            .held
            .as_ref()
            .filter(|k| k.hd == version && k.palette == palette);
        self.images = parts
            .iter()
            .map(|part| {
                let px = &part.pixels;
                let since = match (part.role, held) {
                    (Role::Static, Some(k)) => {
                        let shift = ((k.centre.0 - centre.0) * 16, (k.centre.1 - centre.1) * 16);
                        px.changed_since(&k.statics, shift).map(|r| (shift, r))
                    }
                    (Role::Frame, Some(k)) => {
                        px.changed_since(&k.frame, (0, 0)).map(|r| ((0, 0), r))
                    }
                    _ => None,
                };
                let mut rgba = Vec::new();
                let patch = match since {
                    Some((shift, rects)) => Some(Patch {
                        shift,
                        rects: rects
                            .into_iter()
                            .map(|r| {
                                let mut words = Vec::new();
                                hd.compose_layer(engine, &px.rect(r), &mut words);
                                (r, words)
                            })
                            .collect(),
                    }),
                    None => {
                        hd.compose_layer(engine, px, &mut rgba);
                        None
                    }
                };
                LayerImage {
                    width: px.width,
                    height: px.height,
                    rgba,
                    patch,
                }
            })
            .collect();
        self.made += 1;
        self.pending = slide.as_ref().and_then(|s| {
            let part = |role| s.parts.iter().find(|p| p.role == role);
            let (statics, frame) = (part(Role::Static)?, part(Role::Frame)?);
            Some(Kept {
                made: self.made,
                hd: version,
                palette,
                centre,
                statics: Rc::clone(&statics.pixels),
                frame: Rc::clone(&frame.pixels),
            })
        });
        self.slide = slide;
    }

    /// Where the layers are drawn for `view`, as [`Game::slide`] gives them.
    fn placements(&self, view: &View) -> Vec<i32> {
        let Some(slide) = &self.slide else {
            return Vec::new();
        };
        u6_core::gfx::motion::slide_placements(slide, view)
            .into_iter()
            .flat_map(|(i, x, y)| [i as i32, x, y])
            .collect()
    }
}

/// Spec 20: smooth motion's side of the page: the switch, the presenter of the engine's scenes,
/// the display's frame interval and the frame a slide is drawn in.
struct Motion<T> {
    feet: Footsteps,
    attacks: u6_render::motion::attacks::Attacks,
    attack_barrier: Option<f64>,
    attack_suspended: bool,
    poses: Vec<Pose>,
    world: bool,
    previewing: bool,
    sliding: bool,
    on: bool,
    #[cfg(test)]
    cadence: bool,
    presenter: Presenter<T>,
    /// The display's frame interval in wall ms, smoothed over the frames ([`Motion::frame`]).
    interval: f64,
    /// The last actual presentation time. Tick observations must not add the pacer's
    /// not-yet-debited frame carry, or settle backwards before this displayed position.
    presented_ms: f64,
    /// The last present drew a slide.
    drawn: bool,
    /// What it drew ([`Drawn`]).
    shown: Option<Drawn<T>>,
    frame: Framebuffer,
    /// A scene that snaps was published since [`Game::slide_view`] last looked: the slide's
    /// layers the page holds are of scenes before it, and are dropped as the base of the next
    /// changes, whatever made it snap (a load, and a dissolve or an XOR plot too: their next
    /// layers come whole).
    snapped: bool,
    /// An early picture was handed and no scene of the engine's since ([`Motion::old_cell`]).
    early: bool,
    /// Spec 20 Design 3 (round 13, the receipts): a foreign publication came after the early
    /// picture with no scene since, so the engine's frame shows the map as the engine published it
    /// before the leader's step: the party on its old cell. Ends at the next scene.
    old_cell: bool,
    /// Every publication handed to the presenter: its engine time, whether it was a preview and
    /// whether it had a scene (the tests read it).
    #[cfg(test)]
    taken: Vec<(f64, bool, bool)>,
}

impl<T: AsRef<Scene>> Motion<T> {
    fn new() -> Self {
        Self {
            on: false,
            feet: Footsteps::default(),
            attacks: Default::default(),
            attack_barrier: None,
            attack_suspended: false,
            poses: Vec::new(),
            world: false,
            previewing: false,
            sliding: false,
            #[cfg(test)]
            cadence: true,
            presenter: Presenter::new(),
            interval: 1000.0 / 60.0,
            presented_ms: 0.0,
            drawn: false,
            shown: None,
            frame: Framebuffer::new(),
            snapped: false,
            early: false,
            old_cell: false,
            #[cfg(test)]
            taken: Vec::new(),
        }
    }

    /// A display frame `dt` wall ms after the previous one: the interval follows it, as the
    /// page's own does (a frame past the pacer's cap is a stall, not the display's interval).
    fn frame(&mut self, dt: f64) {
        if dt > 0.0 && dt <= pacer::CAP_MS {
            self.interval += (dt - self.interval) / 8.0;
        }
    }

    /// Forgets every scene and slide (a new game, the switch).
    fn clear(&mut self) {
        self.presenter.clear();
        self.feet.clear();
        self.attacks.clear();
        self.attack_barrier = None;
        self.poses.clear();
        self.world = false;
        self.previewing = false;
        self.sliding = false;
        self.presented_ms = 0.0;
        self.early = false;
        self.old_cell = false;
    }

    /// Hands the presenter `publications` (a scene, or none for a foreign picture, at an engine
    /// time; a preview of the leader's step flagged), slides shown at the pace `rate` the frame
    /// ran at ([`slides_shown`]); with the engine's command prompt up (`prompt`) a held preview
    /// ends ([`Presenter::prompt`]); then the slides as drawn at presentation time `t` while one
    /// runs; the slides ended at `t` are dropped.
    fn publish(
        &mut self,
        publications: impl IntoIterator<Item = (Option<Rc<T>>, f64, bool)>,
        rate: Option<f64>,
        prompt: bool,
        t: f64,
    ) -> Option<View> {
        // A finite speed change clears the pacer's carry before the next engine tick.
        // Sample from the last actual display until raw time catches up; publication and
        // observation timestamps remain the engine's original times.
        let t = t.max(self.presented_ms);
        let slides = if self.presenter.walking() || self.presenter.active(t) {
            rate.is_some_and(|r| r.is_finite() && r > 0.0)
        } else {
            slides_shown(rate, self.interval)
        };
        for (shot, time, preview) in publications {
            #[cfg(test)]
            self.taken.push((time, preview, shot.is_some()));
            self.snapped |= shot.as_ref().is_some_and(|s| (**s).as_ref().snap);
            let foreign = shot.is_none();
            if foreign || self.snapped {
                self.feet.clear();
                self.attacks.clear();
            }
            self.old_cell = foreign && self.early;
            self.early = if foreign { self.early } else { preview };
            let snaps = self.presenter.snaps();
            match shot {
                Some(shot) if preview => {
                    if self.presenter.accept_preview(shot, time, slides) {
                        self.world = true;
                        self.previewing = true;
                    }
                }
                Some(shot) => {
                    self.presenter.publish(shot, time, slides);
                    self.world = true;
                    self.previewing = false;
                }
                None => {
                    self.presenter.foreign();
                    self.world = false;
                }
            }
            if self.presenter.snaps() != snaps {
                self.attacks.clear();
                self.attack_barrier = Some(time);
            }
        }
        // Max/Instant ends existing tracks even without a publication. A finite pace
        // that suppresses a new short tap still lets an earlier held walk settle on release.
        if rate.is_none() {
            self.presenter.snap();
            self.attacks.clear();
            self.attack_barrier = Some(t);
        }
        if prompt {
            self.presenter.prompt();
        }
        let view = self.presenter.view(t).filter(|_| self.presenter.active(t));
        self.presenter.end(t);
        self.presented_ms = t;
        view
    }
}

impl Motion<Shot> {
    /// [`Motion::publish`] of `engine`'s publications ([`Engine::take_motion`]), with its command
    /// prompt as it stands ([`u6_core::engine::command_prompt`]).
    fn take(&mut self, engine: &mut Engine, rate: Option<f64>, t: f64) -> Option<View> {
        let factor = engine.io().playback_speed().code();
        let duration = if factor == 0 {
            0.0
        } else {
            u6_render::motion::SLIDE_MS / f64::from(factor)
        };
        self.presenter.set_duration(duration);
        self.presenter.set_minimum_duration(
            rate.map_or(0.0, |r| r * self.interval * u6_render::motion::MIN_FRAMES),
        );
        let rate = if factor == 0 {
            None
        } else if self.presenter.walking() {
            rate
        } else {
            rate.map(|r| r * f64::from(factor))
        };
        let taken = engine.take_motion();
        let prompt = u6_core::engine::command_prompt(&engine.state());
        let view = self.publish(
            taken
                .into_iter()
                .map(|p| (p.shot, p.time_ms as f64, p.preview)),
            rate,
            prompt,
            t,
        );
        for (at, strike) in engine.io().take_attacks() {
            if !self.attack_suspended
                && self.attack_barrier.is_none_or(|b| at as f64 > b)
                && self.world
                && rate.is_some()
            {
                self.attacks.start(strike, at as f64, factor);
            }
        }
        if rate.is_none() {
            self.attacks.clear();
        }
        view
    }
}

/// The whole save after a Save since generation `since`: `savegame/objlist` as the last Save (or
/// `u`'s character) wrote it and every block file as the game holds it (the files a Save
/// commits; between Saves they do not change), under the original's names, and the remembered
/// sight the last Save wrote (spec 18, [`SIGHT_FILE`]), if it wrote one. Empty when nothing was
/// saved since `since`.
fn whole_save(s: &u6_core::State, since: u32) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    if s.saveload.generation <= since {
        return out;
    }
    if let Some(objlist) = &s.saveload.objlist {
        out.insert(OBJLIST_FILE.to_string(), objlist.clone());
    }
    if let Some(sight) = &s.saveload.sight {
        out.insert(SIGHT_FILE.to_string(), sight.bytes().to_vec());
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
        | InputEvent::Paced(_)
        | InputEvent::Release(_)
        | InputEvent::Text(_) => None,
    })
}

/// Queues a pointer event. Spec 22 Contract 5: a move replaces the move queued last when both
/// keep the same buttons (the game samples the pointer once per frame and takes a run of moves
/// at once anyway): the event queued last is a pointer event with the new one's buttons, and the
/// pointer had them before it too (the pointer event queued before it, else `handed`, the last
/// one the engine was handed). A press or a release is never replaced, so it stays where its own
/// event put it, and the moves after it follow it. Spec 17: nor is one of the first `kept` (a
/// dismissal of the menu stands after them, [`Game::dismiss_menu`]).
fn queue_pointer(
    pending: &mut Vec<InputEvent>,
    kept: usize,
    handed: u8,
    x: i32,
    y: i32,
    buttons: u8,
) {
    let word = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    let buttons = buttons & 3;
    let ev = InputEvent::Pointer {
        x: word(x),
        y: word(y),
        buttons,
    };
    if pending.len() > kept
        && let Some((InputEvent::Pointer { buttons: last, .. }, before)) = pending.split_last()
    {
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
            "{{\"object\":{},\"kind\":{kind},\"entries\":[",
            section.object
        ));
        for (j, e) in section.entries.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&entry_json(e));
        }
        out.push_str("]}");
    }
    let go_here = m
        .go_here
        .as_ref()
        .map_or_else(|| "null".to_string(), entry_json);
    out.push_str(&format!("],\"go_here\":{go_here}}}"));
    out
}

/// One entry of [`menu_json`]: `{code, enabled, reason, theft}`.
fn entry_json(e: &u6_core::ui::menu::Entry) -> String {
    let reason = e
        .reason
        .map_or_else(|| "null".to_string(), |r| format!("\"{}\"", r.code()));
    format!(
        "{{\"code\":{},\"enabled\":{},\"reason\":{reason},\"theft\":{}}}",
        e.code,
        e.enabled(),
        e.theft
    )
}

/// [`Game::menu_view`]'s JSON for `v`.
fn view_json(v: &u6_core::ui::popup::View, [paper, ink, head]: [u32; 3]) -> String {
    use u6_core::ui::popup::RowKind;
    let wh = |(x0, y0, x1, y1): (i32, i32, i32, i32)| {
        format!("[{x0},{y0},{},{}]", x1 - x0 + 1, y1 - y0 + 1)
    };
    let mut out = format!(
        "{{\"rect\":{},\"text\":{},\"hover\":{},\"korean\":{},\"colours\":[{paper},{ink},{head}],\"rows\":[",
        wh(v.rect),
        wh(v.text),
        v.hover,
        v.korean
    );
    for (i, r) in v.rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let kind = match r.kind {
            RowKind::Head => "head",
            RowKind::Entry => "entry",
            RowKind::Note => "note",
        };
        let code = r.code.map_or_else(|| "null".to_string(), |c| c.to_string());
        let reason = r
            .reason
            .map_or_else(|| "null".to_string(), |r| format!("\"{}\"", r.code()));
        out.push_str(&format!(
            "{{\"kind\":\"{kind}\",\"code\":{code},\"enabled\":{},\"reason\":{reason},\"theft\":{},\"y\":{}}}",
            r.enabled(),
            r.theft,
            r.y
        ));
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

    #[test]
    fn control_edges_keep_their_places_when_pointer_moves_are_coalesced() {
        let mut game = Game::new();
        game.engine = Some(Engine::with_game(
            u6_core::State::empty(),
            Files::default(),
            |_, io| async move { io.vblank().await },
        ));
        game.pointer(10, 10, 0);
        game.set_control_action(true);
        game.pointer(11, 11, 0);
        game.pointer(12, 12, 0);
        assert_eq!(
            game.pending.len(),
            2,
            "moves after the Ctrl edge coalesce only with each other"
        );
        assert_eq!(game.controls, [(1, true)]);
        game.pointer(12, 12, 1);
        game.pointer(12, 12, 0);
        game.release_keys();
        assert_eq!(
            game.controls,
            [(1, true), (4, false)],
            "focus loss after the click stays after it"
        );
        game.pointer(14, 14, 0);
        assert_eq!(
            game.pending.len(),
            5,
            "the release edge protects the preceding pointer event"
        );
        game.step(20);
        assert!(game.controls.is_empty());
        assert!(!game.control_action);
    }

    #[test]
    fn a_held_key_cooldown_requires_a_quiet_command_prompt() {
        let mut game = Game::new();
        assert!(!game.command_prompt(), "no engine");
        for reading in [0, 1] {
            for accept in [false, true] {
                for targeting in [0, 1, 2] {
                    let mut state = u6_core::State::empty();
                    state.main.reading = reading;
                    state.ui.input.accept = accept;
                    state.ui.input.targeting = targeting;
                    game.engine = Some(Engine::with_game(
                        state,
                        Files::default(),
                        |_, io| async move { io.vblank().await },
                    ));
                    assert_eq!(game.at_prompt(), reading == 1, "the weaker read flag");
                    assert_eq!(
                        game.command_prompt(),
                        reading == 1 && accept && targeting == 0,
                        "only a command read accepts a paced direction"
                    );
                }
            }
        }
    }
    use u6_core::geom::{HALF_H, HALF_W};
    use u6_testkit::{Quiet, assert_same};

    #[test]
    fn turn_speed_defaults_to_original_and_survives_replacement_of_the_engine() {
        let mut g = Game::new();
        assert_eq!(g.turn_speed(), 1);
        for factor in [2, 4, 0, 1, 99] {
            g.set_turn_speed(factor);
            let expected = if factor == 99 { 1 } else { factor };
            let e = Engine::with_game(u6_core::State::empty(), Files::default(), |_, _| async {});
            g.put_on_screen(e);
            assert_eq!(g.turn_speed(), expected);
            assert_eq!(
                g.engine.as_ref().unwrap().io().turn_speed().code(),
                expected
            );
            assert_eq!(g.speed(), 1.0, "turn speed is separate from game speed");
        }
    }

    /// Spec 22 Contract 5: a run of moves keeping the buttons collapses to its last; a press or
    /// a release is never replaced, so the move after it, the held move after a press included,
    /// is queued behind it (until round 3 of spec 22 the press moved with that held move to
    /// (21, 31), and a release to the hover after it).
    #[test]
    fn pointer_moves_collapse_and_button_changes_queue() {
        let mut q = Vec::new();
        let p = |x, y, buttons| InputEvent::Pointer { x, y, buttons };
        queue_pointer(&mut q, 0, 0, 10, 10, 0);
        queue_pointer(&mut q, 0, 0, 20, 30, 0);
        queue_pointer(&mut q, 0, 0, 20, 30, 1);
        queue_pointer(&mut q, 0, 0, 21, 31, 5);
        queue_pointer(&mut q, 0, 0, 21, 31, 0);
        assert_eq!(q, [p(20, 30, 0), p(20, 30, 1), p(21, 31, 1), p(21, 31, 0)]);
        q.push(InputEvent::Key(Key::Space));
        queue_pointer(&mut q, 0, 0, 99_999, -99_999, 2);
        assert_eq!(
            q.last(),
            Some(&p(i16::MAX, i16::MIN, 2)),
            "a key in between keeps both"
        );
        // A move right after one a dismissal stands after (spec 17: `kept`) is queued after it.
        let n = q.len();
        queue_pointer(&mut q, n, 0, 7, 7, 2);
        assert_eq!(q.len(), n + 1, "kept: not replaced");
        queue_pointer(&mut q, n, 0, 8, 8, 2);
        assert_eq!(q[n..], [p(8, 8, 2)], "past it: replaced as before");
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
                queue_pointer(&mut q, 0, handed, x, y, b);
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

    /// Spec 22 Contract 5 "Positions": a relaunch starts the page's stream again (`put_on_screen`:
    /// no button handed to the new engine): a press the old engine took keeps no two moves apart
    /// after it.
    #[test]
    fn a_press_handed_before_a_relaunch_keeps_no_moves_apart_after_it() {
        let idle = || {
            Engine::with_game(
                u6_core::State::empty(),
                Files::default(),
                |_, io| async move {
                    io.sleep(u32::MAX).await;
                },
            )
        };
        let mut game = Game::new();
        game.put_on_screen(idle());
        game.pointer(40, 50, 1);
        game.step(16);
        assert_eq!(game.handed, 1, "the old engine took the press");
        game.put_on_screen(idle());
        game.pointer(1, 1, 0);
        game.pointer(2, 2, 0);
        let p = |x, y, buttons| InputEvent::Pointer { x, y, buttons };
        assert_eq!(game.pending, [p(2, 2, 0)], "the moves collapse");
    }

    /// Spec 22 Contract 6: `talk_target` codes the read's hot and pressed targets for the page.
    #[test]
    fn talk_target_codes_the_hot_and_pressed_targets() {
        use u6_core::ui::talk::{TalkClicks, Target};
        let mut game = Game::new();
        assert_eq!(game.talk_target(), [-1, -1], "before a start");
        let with = |talk: TalkClicks| {
            let mut s = u6_core::State::empty();
            s.ui.talk = talk;
            Some(Engine::with_game(s, Files::default(), |_, io| async move {
                io.sleep(u32::MAX).await;
            }))
        };
        game.engine = with(TalkClicks {
            reading: true,
            buttons: Vec::new(),
            hot: Some(Target::Button(1)),
            pressed: Some(Target::Key { row: 2, col: 3 }),
        });
        assert_eq!(game.talk_target(), [1, 0x10203]);
        game.engine = with(TalkClicks::default());
        assert_eq!(game.talk_target(), [-1, -1], "outside a read");
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
        assert!(game.key_down("ArrowUp", "ArrowUp", false, 0.0));
        assert!(!game.key_down("ShiftLeft", "Shift", false, 0.0));
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
        assert!(game.key_down("KeyS", "s", true, 0.0));
        assert!(game.key_repeat("KeyS", "s", true, 0.0));
        assert!(game.key_repeat("KeyS", "s", true, 0.0));
        assert!(game.key_repeat("KeyS", "s", false, 0.0));
        assert!(game.key_down("ArrowUp", "ArrowUp", false, 0.0));
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
        assert!(game.key_repeat("ArrowUp", "ArrowUp", false, 0.0));
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
        game.key_down("KeyA", "a", false, 0.0);
        game.text_input("가나");
        game.key_down("Enter", "Enter", false, 0.0);
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

    /// Spec 18 (saved with the game): the remembered sight's stored file is taken up to the
    /// largest it can be (`MAX_SIGHT_FILE_BYTES`), past a block's limit, and not counted in the
    /// save's total: with the total used up it is still taken; one byte more is left out; in
    /// another case of its name it replaces the one taken. The page reads the same limits
    /// (synthetic bytes).
    #[test]
    fn the_sight_file_has_its_own_limit_beside_the_saves() {
        let mut game = Game::new();
        for i in 0..80 {
            game.load_save(&format!("savegame/x{i:03}"), vec![0; MAX_SAVE_FILE_BYTES]);
        }
        let used = game.save_bytes;
        assert!(
            used + MAX_SAVE_FILE_BYTES > MAX_SAVE_BYTES,
            "the total used up"
        );
        assert!(game.load_save("savegame/u6sight", vec![1; MAX_SIGHT_FILE_BYTES]));
        assert!(game.save_bytes == used, "not counted in the total");
        assert!(!game.load_save("savegame/u6sight", vec![2; MAX_SIGHT_FILE_BYTES + 1]));
        assert!(game.saves[SIGHT_FILE][0] == 1, "the one taken stays");
        assert!(game.load_save("SAVEGAME/U6SIGHT", vec![3; 10]));
        let sights: Vec<&String> = game.saves.keys().filter(|p| is_sight(p)).collect();
        assert!(sights == ["SAVEGAME/U6SIGHT"], "one sight file");
        assert!(max_save_file_bytes_of("savegame/U6Sight") == MAX_SIGHT_FILE_BYTES as f64);
        assert!(max_save_file_bytes_of("savegame/objblkAA") == MAX_SAVE_FILE_BYTES as f64);
        assert!(is_sight_file("savegame/u6sight") && !is_sight_file("savegame/objlist"));
    }

    /// Spec 18 (saved with the game): after the stored save was removed, the whole save a Save
    /// lists holds the remembered sight the last Save wrote, beside the objlist (synthetic state).
    #[test]
    fn the_whole_save_holds_the_sight_file_the_last_save_wrote() {
        let mut s = u6_core::State::empty();
        s.saveload.generation = 1;
        s.saveload.objlist = Some(vec![1; 4]);
        assert!(
            !whole_save(&s, 0).contains_key(SIGHT_FILE),
            "none written: none listed"
        );
        s.saveload.sight = Some(u6_core::saveload::SavedSight::new(vec![5; 3]));
        s.saveload.sight_gen = 1;
        let whole = whole_save(&s, 0);
        assert!(whole.get(SIGHT_FILE).is_some_and(|b| b == &[5; 3]));
        assert!(whole.contains_key(OBJLIST_FILE));
        assert!(whole_save(&s, 1).is_empty(), "nothing saved since");
    }

    /// Spec 18 (saved with the game): the character `u` writes a new game's objlist over a Save's
    /// (`u6_core::intro::handoff::apply`) and lets the Save's remembered sight go, bound to the
    /// objlist it replaced: the whole save listed after the stored save was removed holds `u`'s
    /// objlist and no sight file (synthetic state).
    #[test]
    fn after_u_the_whole_save_holds_no_sight_file() {
        let mut s = u6_core::State::empty();
        s.actors.party.names = vec![String::new(); 16];
        s.saveload.generation = 1;
        s.saveload.objlist = Some(vec![1; 4]);
        s.saveload.sight = Some(u6_core::saveload::SavedSight::new(vec![5; 3]));
        s.saveload.sight_gen = 1;
        assert!(whole_save(&s, 0).contains_key(SIGHT_FILE), "after the Save");
        let new_game = vec![0; 0x1c73];
        u6_core::intro::handoff::apply(&mut s, &new_game);
        let whole = whole_save(&s, 1);
        assert!(
            whole.get(OBJLIST_FILE) == Some(&new_game),
            "after u: u's objlist"
        );
        assert!(!whole.contains_key(SIGHT_FILE), "after u: no sight file");
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
                game.key_down("KeyT", "t", false, 0.0);
                game.key_down("Minus", "-", false, 0.0);
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
                    0.0,
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

    /// On the user's zip (skipped when absent; read only): the disk entries declare well under
    /// the unzip limit, the start keeps every game file in place, and those files given back as
    /// loose files start the same game. The second start runs with HD on at 4× (spec 07 HD0):
    /// the start moves the page's version and composes its first HD frame whole, and the frames
    /// after it are the definition's.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn real_zip_keeps_its_game_files_and_restarts_from_them() {
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the archive (U6_ARCHIVE) not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
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

    /// Spec 23 on the user's zip (read only; prints whether it ran, skips when the zip is absent
    /// and fails then with `U6_REQUIRE_ARCHIVE=1`): passes at the prompt fill the message window
    /// past its rows; scrolled back, a press on its bar is the history's until the release: the
    /// game's handler sees no button, the pace runs at 1× as with one held, and the speed again
    /// from the step that takes the release (queued, not yet stepped) on.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files and the environment
    fn a_press_the_message_history_holds_paces_at_1x() {
        if !u6_testkit::archive().exists() {
            let required = std::env::var("U6_REQUIRE_ARCHIVE").is_ok_and(|v| v == "1");
            assert!(
                !required,
                "U6_REQUIRE_ARCHIVE=1: the archive (U6_ARCHIVE) is required"
            );
            println!("history pace: skipped (the archive (U6_ARCHIVE) not present)");
            return;
        }
        println!("history pace: ran");
        let mut game = Game::new();
        game.collect(
            "ultima-vi.zip",
            std::fs::read(u6_testkit::archive()).quiet(),
        )
        .quiet();
        game.boot(1, true).quiet();
        let prompt = |game: &mut Game| {
            for _ in 0..2000 {
                if game.at_prompt() {
                    return;
                }
                game.step(20);
            }
            panic!("the prompt");
        };
        prompt(&mut game);
        for _ in 0..14 {
            game.key_down("Space", " ", false, 0.0);
            game.key_up("Space");
            game.step(20);
            prompt(&mut game);
        }
        assert!(game.wheel(200, 150, -3), "over the message window");
        assert_eq!(game.history_offset(), 3);
        game.set_speed(8.0);
        let fast = game.pace(100.0);
        game.ran(fast);
        assert!(fast >= 40, "{fast} steps free");
        // On the bar's track, above the thumb (a page older): the press the history's.
        game.pointer(310, 113, 1);
        game.step(20);
        let engine = game.engine.as_ref().unwrap();
        assert!(engine.history_holding(), "the history's press");
        assert_eq!(
            engine.io().handler_pointer().buttons,
            0,
            "no button for the game"
        );
        assert!(game.pending.is_empty());
        assert_eq!(game.pace(100.0), 5, "held: 1×");
        game.ran(5);
        game.pointer(310, 113, 0);
        // The release queued, the press still the history's: the step that takes it is paced as
        // released.
        assert!(game.engine.as_ref().unwrap().history_holding());
        assert_eq!(game.pace(100.0), fast, "the release queued: the speed");
        game.ran(fast);
        game.step(20);
        assert!(!game.engine.as_ref().unwrap().history_holding());
        assert_eq!(game.pace(100.0), fast, "released: the speed");
    }

    /// On the user's zip (skipped when absent; read only): a start gives every buffer the source
    /// plane (spec 07 HD0), the frames name images (counted, never printed), and the frames and
    /// the page's RGBA are those of an engine without the plane, step by step.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn a_start_keeps_the_source_plane_and_the_same_frames() {
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the archive (U6_ARCHIVE) not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
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
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the archive (U6_ARCHIVE) not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
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
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the archive (U6_ARCHIVE) not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
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
        game.key_down("ArrowRight", "ArrowRight", false, 0.0);
        // A tap: a key still down is pressed again at every prompt (spec 19 O1c).
        game.key_up("ArrowRight");
        ticks(&mut game, 400);
        let moved = game.leader_position();
        assert!(moved != start, "one step east");
        game.key_down("KeyS", "s", true, 0.0);
        ticks(&mut game, 30);
        game.key_down("KeyY", "y", false, 0.0);
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
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the archive (U6_ARCHIVE) not present");
            return;
        }
        let ticks = |game: &mut Game, n: u32| {
            for _ in 0..n {
                game.step(16);
                game.present();
            }
        };
        let save = |game: &mut Game| {
            game.key_down("KeyS", "s", true, 0.0);
            ticks(game, 30);
            game.key_down("KeyY", "y", false, 0.0);
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
            .collect(
                "ultima-vi.zip",
                std::fs::read(u6_testkit::archive()).quiet(),
            )
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
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the archive (U6_ARCHIVE) not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
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
        game.key_down(&format!("Key{}", key.to_uppercase()), key, false, 0.0);
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

    /// Text diagnostics follow the visible pixel font without a rendering switch. HD scale
    /// changes and recolouring publish both pixels and metadata without advancing the engine.
    #[test]
    fn pixel_text_diagnostics_follow_the_frame_and_only_changed_ink_is_dirty_without_an_overlay() {
        let mut game = Game::new();
        game.put_on_screen(text_engine());
        press(&mut game, "t");
        for scale in [2, 4] {
            game.set_hd(true, scale);
            let e = game.engine.as_ref().unwrap();
            let rgb = |i: usize| {
                let c = e.frame().palette[i];
                u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
            };
            let (ink, paper) = (rgb(3), rgb(4));
            assert_eq!(
                game.hd_glyphs(),
                vec![16, 16, 0x41, ink, paper, 24, 16, 0x7a, ink, paper]
            );
            assert_eq!(
                game.hd_frame_rgba(),
                expected_hd(&game, Scale::from_factor(scale))
            );
        }
        game.set_hd(true, 2);
        game.hd_take_dirty();
        let before = game.frame_version();
        press(&mut game, "r");
        assert!(game.frame_version() > before);
        // Synthetic A has row 0x41: only columns 17 and 23 change ink. No font overlay
        // needs the unchanged leftmost column included in the dirty bounding rectangle.
        assert_eq!(game.hd_take_dirty(), vec![34, 32, 14, 16]);
        assert_eq!(game.hd_frame_rgba(), expected_hd(&game, Scale::X2));
        press(&mut game, "p");
        assert_eq!(game.hd_take_dirty(), vec![38, 40, 2, 2], "one source pixel");
        assert_eq!(
            game.hd_glyphs().len(),
            5,
            "the partly covered letter is no longer whole"
        );
        assert_eq!(game.hd_frame_rgba(), expected_hd(&game, Scale::X2));
        let kept = game.hd_glyphs();
        game.set_hd(false, 2);
        assert!(
            game.hd_glyphs().is_empty(),
            "HD off exposes no stale diagnostics"
        );
        game.set_hd(true, 2);
        assert_eq!(game.hd_glyphs(), kept);
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
    /// [`Game::hd_symbol_rows`]; both empty with HD off.
    #[test]
    fn the_hd_glyphs_give_a_typed_space_and_a_symbol_in_their_ranges() {
        let mut game = Game::new();
        game.put_on_screen(symbol_engine());
        game.step(20);
        game.present();
        game.set_hd(true, 2);

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
        assert_eq!(game.hd_frame_rgba(), expected_hd(&game, Scale::X2));
        game.set_hd(false, 2);
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
    /// The horizontal shift in -16..=16 pixels that best maps the shown map of RGBA frame `a`
    /// onto `b` (most equal pixels over every other row and column; the smallest shift first).
    fn map_shift(a: &[u8], b: &[u8]) -> i32 {
        let (x0, y0, x1, y1) = u6_core::geom::SHOWN;
        let (x0, y0, x1, y1) = (i32::from(x0), i32::from(y0), i32::from(x1), i32::from(y1));
        let w = u6_render::WIDTH as i32;
        let score = |dx: i32| {
            let mut same = 0;
            for y in (y0..=y1).step_by(2) {
                for x in (x0 + 16..=x1 - 16).step_by(2) {
                    let (i, j) = (((y * w + x - dx) * 4) as usize, ((y * w + x) * 4) as usize);
                    same += usize::from(a[i..i + 3] == b[j..j + 3]);
                }
            }
            same
        };
        let mut best = (score(0), 0);
        for d in 1..=16 {
            for dx in [-d, d] {
                let s = score(dx);
                if s > best.0 {
                    best = (s, dx);
                }
            }
        }
        best.1
    }

    /// A synthetic scene for the motion tests: an 11 × 11 view round (x, 0), the leader (slot 1)
    /// drawn on its centre.
    fn walker_scene(x: i32) -> Scene {
        use u6_render::motion::{Cell, Node, Placed, Rank};
        let mut cells = vec![Cell::default(); 121];
        cells[60].nodes.push(Node {
            word: 0x180,
            slot: Some(1),
            rank: Rank::Own,
        });
        Scene {
            leader: Some(1),
            centre: (x, 0, 0),
            origin: (x - 5, -5),
            cols: 11,
            rows: 11,
            corner: (0, 0),
            cells,
            placed: vec![Placed {
                slot: 1,
                cell: (x, 0),
                kind: 0x180,
            }],
            snap: false,
        }
    }

    #[test]
    fn footsteps_do_not_revive_a_foreign_frame_through_a_rejected_preview() {
        for (duration, rate) in [(360.0, None), (0.0, Some(1.0)), (360.0, Some(1000.0))] {
            let mut motion = Motion::<Scene>::new();
            motion.presenter.set_duration(duration);
            motion.publish(
                [(Some(Rc::new(walker_scene(0))), 0.0, false)],
                Some(1.0),
                false,
                0.0,
            );
            assert!(motion.world);
            motion.publish([(None, 20.0, false)], rate, false, 20.0);
            assert!(!motion.world);
            motion.publish(
                [(Some(Rc::new(walker_scene(1))), 40.0, true)],
                rate,
                false,
                40.0,
            );
            assert!(
                !motion.world,
                "an ignored preview cannot expose the previous world scene"
            );
        }
    }

    #[test]
    fn a_finite_rate_change_cannot_rewind_a_running_slide() {
        let mut pacer = Pacer::new();
        let mut motion = Motion::<Scene>::new();
        motion.presenter.set_walking(&[1], (1, 0), 0.0);
        motion.publish(
            [(Some(Rc::new(walker_scene(0))), 0.0, false)],
            Some(1.0),
            false,
            0.0,
        );
        motion.publish(
            [(Some(Rc::new(walker_scene(1))), 20.0, false)],
            Some(1.0),
            false,
            20.0,
        );
        assert_eq!(pacer.due(35.0, false), 1);
        pacer.ran(1);
        let before = motion
            .publish([], pacer.pace_rate(), false, 20.0 + pacer.carried_ms())
            .unwrap();
        assert_eq!(
            before.camera,
            (-15, 0),
            "the actual carried frame is already displayed"
        );
        pacer.set_speed(Speed::Finite(4.0));
        assert_eq!(pacer.carried_ms(), 0.0, "changing speed discards owed time");
        let after = motion
            .publish([], pacer.pace_rate(), false, 20.0 + pacer.carried_ms())
            .unwrap();
        assert_eq!(
            after.camera, before.camera,
            "the same movement session cannot run backwards after a finite speed change"
        );
        assert_eq!(
            motion.presented_ms, 35.0,
            "display time does not rewind with raw engine carry"
        );
        let next = motion.publish([], pacer.pace_rate(), false, 90.0).unwrap();
        assert!(
            next.camera.0 > after.camera.0,
            "motion advances once engine time catches up"
        );
    }

    #[test]
    fn max_ends_an_existing_slide_without_waiting_for_a_publication() {
        for held in [false, true] {
            let mut motion = Motion::<Scene>::new();
            if held {
                motion.presenter.set_walking(&[1], (1, 0), 0.0);
            }
            motion.publish(
                [(Some(Rc::new(walker_scene(0))), 0.0, false)],
                Some(1.0),
                false,
                0.0,
            );
            assert!(
                motion
                    .publish(
                        [(Some(Rc::new(walker_scene(1))), 20.0, false)],
                        Some(1.0),
                        false,
                        100.0
                    )
                    .is_some()
            );
            assert!(
                motion.publish([], None, false, 101.0).is_none(),
                "Max ends the existing slide even with no new publication"
            );
            assert!(motion.presenter.timings().is_empty(), "Max drops tracks");
            assert!(
                motion.publish([], Some(1.0), false, 102.0).is_none(),
                "returning to finite speed cannot resume a cancelled slide"
            );
        }
    }

    /// Spec 20 receipt (timing): a step published at an engine step slides for 360 / s wall ms at
    /// 1×, 2× and 8× on a 60 Hz and a 144 Hz display, within one display frame, measured as the
    /// display frames whose present draws it; a pointer button held runs it at 1×; at Max, and
    /// where 360 / s is under two display frames, no slide.
    #[test]
    fn a_slide_lasts_360_ms_over_the_speed_within_a_display_frame_and_none_at_max() {
        for hz in [60.0, 144.0] {
            let dt = 1000.0 / hz;
            for (speed, held, want) in [
                (Speed::Finite(1.0), false, Some(360.0)),
                (Speed::Finite(2.0), false, Some(180.0)),
                (Speed::Finite(8.0), false, Some(45.0)),
                (Speed::Finite(8.0), true, Some(360.0)),
                (Speed::Finite(12.0), false, (hz > 100.0).then_some(30.0)),
                (Speed::Max, false, None),
            ] {
                let mut pacer = Pacer::new();
                pacer.set_speed(speed);
                let mut motion = Motion::<Scene>::new();
                motion
                    .presenter
                    .publish(Rc::new(walker_scene(0)), 0.0, true);
                let (mut now, mut published, mut drawn) = (0u64, false, 0u32);
                for _ in 0..600 {
                    let k = pacer.due(dt, held).min(8);
                    let mut publications = Vec::new();
                    for _ in 0..k {
                        now += u64::from(pacer::STEP_MS);
                        if !published && now >= 2000 {
                            publications.push((Some(Rc::new(walker_scene(1))), now as f64, false));
                            published = true;
                        }
                    }
                    pacer.ran(k);
                    motion.frame(dt);
                    let t = now as f64 + pacer.carried_ms();
                    let rate = pacer.pace_rate();
                    drawn += u32::from(motion.publish(publications, rate, false, t).is_some());
                }
                assert!(published, "{speed:?}: the step was published");
                let shown = f64::from(drawn) * dt;
                let label = format!("{hz} Hz, {speed:?}, held {held}");
                match want {
                    Some(ms) => assert!(
                        (shown - ms).abs() <= dt,
                        "{label}: {shown} ms shown, {ms} wanted"
                    ),
                    None => assert_eq!(drawn, 0, "{label}: no slide"),
                }
            }
        }
    }

    /// Spec 20 on the user's zip (skipped when absent): with smooth motion on, a step east at 1×
    /// on a 60 Hz display draws in-between pictures (the page's RGBA frame neither the frame
    /// before nor the one after), on presents with no step too ([`Game::sliding`]), each one
    /// frame; the present after the slide shows the engine's frame; and the engine's frames are
    /// those of the same run with it off.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn a_step_slides_in_the_page_and_ends_on_the_engines_frame() {
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the local archive not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
        let ticks = |game: &Game| game.engine.as_ref().map_or(0, Engine::ticks);
        let run = |motion: bool| {
            let mut game = Game::new();
            game.collect("ultima-vi.zip", data.clone()).quiet();
            game.set_motion(motion);
            // Exact original-frame oracle; default B has separate posed-pixel regressions.
            game.set_footstep_pattern(0);
            game.boot(3, true).quiet();
            let dt = 1000.0 / 60.0;
            let mut frames = Vec::new();
            let mut engine_frames = Vec::new();
            let mut sliding = 0;
            let mut no_step = 0;
            let mut frame = |game: &mut Game| {
                let k = game.pace(dt);
                for _ in 0..k {
                    game.step(pacer::STEP_MS);
                }
                game.ran(k);
                let slide = game.sliding();
                if k > 0 || slide {
                    let before = game.frame_version();
                    game.present();
                    no_step += u32::from(k == 0 && game.frame_version() != before);
                }
                sliding += u32::from(slide);
                let e = game.engine.as_ref().quiet();
                engine_frames.push(e.frame().pixels().to_vec());
                frames.push(game.frame_rgba());
            };
            let mut start = None;
            for i in 0..6600 {
                if start.is_none() && (i == 6000 || game.at_prompt() && ticks(&game) > 3000) {
                    game.key_down("ArrowRight", "ArrowRight", false, 0.0);
                    start = Some(i);
                }
                if start.is_some_and(|s| i >= s + 600) {
                    break;
                }
                frame(&mut game);
            }
            let start = start.quiet();
            (
                frames.split_off(start),
                engine_frames,
                sliding,
                no_step,
                game,
            )
        };
        let (on, on_engine, sliding, no_step, game) = run(true);
        let (off, off_engine, _, _, _) = run(false);
        assert_same!(on_engine, off_engine, "the engine's frames");
        // The horizontal shift (pixels) that best maps the shown map of the frame before the key
        // onto each frame; in between: neither 0 nor a whole cell.
        let in_between = |frames: &[Vec<u8>]| {
            let shifts: Vec<i32> = frames.iter().map(|f| map_shift(&frames[0], f)).collect();
            let between = shifts.iter().filter(|d| d.abs() % 16 != 0).count();
            (between, shifts)
        };
        let ((between, shifts), (between_off, shifts_off)) = (in_between(&on), in_between(&off));
        assert!(
            shifts.contains(&-16) && shifts_off.contains(&-16),
            "a step east"
        );
        let last = on.last().quiet();
        assert!(
            sliding > 0 && no_step > 0,
            "presents with no step drew the slide"
        );
        assert!(
            between > 0 && between_off == 0,
            "in-between pictures: {between}, off {between_off}"
        );
        let mut rgba = Vec::new();
        game.engine.as_ref().quiet().frame().write_rgba(&mut rgba);
        assert_same!(*last, rgba, "the slide ended on the engine's frame");
        assert!(!game.sliding(), "no slide left");
        let mut seen = shifts.clone();
        seen.dedup();
        println!(
            "a step east at 1x: {between} frames in between (off {between_off}), {sliding} \
             presents sliding, {no_step} with no step; the map's shift by frame: {seen:?}"
        );
    }
    /// Timings only (run with `--ignored --nocapture`, gate profile; native, not wasm): with
    /// smooth motion on and HD on at 4×, the wall time of each present while a step east slides
    /// (the slide's layers placed, and made once a publication: none taken by a page here, so made
    /// whole), against the presents of the same frames with smooth motion off.
    #[test]
    #[ignore]
    #[allow(clippy::disallowed_methods, clippy::disallowed_types)] // reads the zip; a timing
    fn bench_the_presents_of_a_slide_at_hd_4x() {
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the local archive not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
        for motion in [true, false] {
            let mut game = Game::new();
            game.collect("ultima-vi.zip", data.clone()).quiet();
            game.set_motion(motion);
            game.set_hd(true, 4);
            game.boot(3, true).quiet();
            let dt = 1000.0 / 60.0;
            let mut times = Vec::new();
            let mut pressed = None;
            for i in 0..6600 {
                let ticks = game.engine.as_ref().map_or(0, Engine::ticks);
                if pressed.is_none() && (i == 6000 || game.at_prompt() && ticks > 3000) {
                    game.key_down("ArrowRight", "ArrowRight", false, 0.0);
                    pressed = Some(i);
                }
                if pressed.is_some_and(|p| i >= p + 120) {
                    break;
                }
                let k = game.pace(dt);
                for _ in 0..k {
                    game.step(pacer::STEP_MS);
                }
                game.ran(k);
                if k > 0 || game.sliding() {
                    let slide = game.sliding();
                    let t = std::time::Instant::now();
                    game.present();
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    if pressed.is_some() && (slide || !motion) {
                        times.push(ms);
                    }
                }
            }
            times.sort_by(f64::total_cmp);
            let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
            println!(
                "motion {motion}: {} presents after the key, median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                times.len(),
                at(0.5),
                at(0.95),
                at(1.0)
            );
        }
    }
    /// One canvas of the page: its size in device pixels and its words.
    type Canvas = (usize, usize, Vec<u32>);

    /// How the page reports the layers it took ([`Game::slide_applied`]) in a case.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Ack {
        /// At once, as site/app.js does.
        Now,
        /// Twice.
        Twice,
        /// Never (lost).
        Never,
        /// Held back until the case sends it ([`Page::late`]).
        Late,
    }

    /// The page's side of spec 20 Design 5 as site/app.js's drawSlide and takeLayer do it, through
    /// the exports only (sizes, the patch metadata [`Game::slide_layer_patch`], the pixels at
    /// [`Game::slide_layer_ptr`] / [`Game::slide_patch_ptr`] and their lengths): the layers it
    /// holds as canvases, taken whole or as their changes over those it holds (moved, then the
    /// rects put), reported held as `ack` says; with `check_base` off it applies changes whatever
    /// layers they were made from (a safeguard removed).
    struct Page {
        held: Option<(f64, usize, Vec<Canvas>)>,
        ack: Ack,
        check_base: bool,
        late: Vec<f64>,
        /// Takes of the static layer whole and as changes, and the takes that found changes of
        /// layers it does not hold (the slide not drawn that frame, the layers asked for whole).
        whole: usize,
        changes: usize,
        misses: usize,
    }

    /// The bytes `len` from `ptr` (an export's address and length).
    fn exported<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
        if len == 0 {
            return &[];
        }
        // The game's export: `len` bytes at `ptr`, read before the game changes again.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }

    fn words_of(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    }

    /// The poison the page's map is filled with before a slide is drawn over it (no layer word).
    const POISON: u32 = 0x00ab_cdef;

    impl Page {
        fn new() -> Self {
            Self {
                held: None,
                ack: Ack::Now,
                check_base: true,
                late: Vec::new(),
                whole: 0,
                changes: 0,
                misses: 0,
            }
        }

        /// Layer `i` at scale `s` into a canvas: whole, or its changes over `old` when the page
        /// `holds` the layers they apply to; `None` when it does not.
        fn take_layer(
            game: &Game,
            i: u32,
            s: usize,
            old: Option<&Canvas>,
            holds: bool,
        ) -> Option<(Canvas, bool)> {
            let size = game.slide_layer_size(i);
            let (w, h) = (size[0] as usize * s, size[1] as usize * s);
            let patch = game.slide_layer_patch(i);
            if patch.is_empty() {
                let bytes = exported(game.slide_layer_ptr(i), game.slide_layer_len(i));
                let words = if bytes.len() == w * h * 4 {
                    words_of(bytes)
                } else {
                    vec![0; w * h]
                };
                return Some(((w, h, words), true));
            }
            let (_, _, old) = old.filter(|o| holds && (o.0, o.1) == (w, h))?;
            let (dx, dy) = (patch[0] as i64 * s as i64, patch[1] as i64 * s as i64);
            // Moved with `copy`: nothing of the old pixels outside the moved ones.
            let mut words = vec![0u32; w * h];
            for y in 0..h as i64 {
                for x in 0..w as i64 {
                    let (ox, oy) = (x - dx, y - dy);
                    if (0..w as i64).contains(&ox) && (0..h as i64).contains(&oy) {
                        words[y as usize * w + x as usize] = old[oy as usize * w + ox as usize];
                    }
                }
            }
            for k in 0..patch[2] as usize {
                let r = &patch[3 + 4 * k..7 + 4 * k];
                let [x, y, rw, rh] = [r[0], r[1], r[2], r[3]].map(|v| v as usize * s);
                let bytes = exported(
                    game.slide_patch_ptr(i, k as u32),
                    game.slide_patch_len(i, k as u32),
                );
                if bytes.len() == rw * rh * 4 {
                    let rect = words_of(bytes);
                    for row in 0..rh {
                        words[(y + row) * w + x..(y + row) * w + x + rw]
                            .copy_from_slice(&rect[row * rw..(row + 1) * rw]);
                    }
                }
            }
            Some(((w, h, words), false))
        }

        /// One display frame after [`Game::present`]: takes the layers when they are new, as
        /// drawSlide does (with HD on: off, the page shows the RGBA frame); whether a slide is
        /// drawn.
        fn take(&mut self, game: &mut Game) -> bool {
            if !game.hd_on() || game.slide().is_empty() {
                return false;
            }
            let (version, s) = (game.slide_layers_version(), game.hd_scale() as usize);
            if self.held.as_ref().map(|h| (h.0, h.1)) == Some((version, s)) {
                return true;
            }
            let (kept, was) = match self.held.take() {
                Some((v, at, canvases)) if at == s => (canvases, Some(v)),
                _ => (Vec::new(), None),
            };
            let holds =
                !kept.is_empty() && (!self.check_base || was == Some(game.slide_layers_base()));
            let mut canvases = Vec::new();
            for i in 0..game.slide_layer_count() {
                let Some((canvas, whole)) =
                    Self::take_layer(game, i, s, kept.get(i as usize), holds)
                else {
                    game.slide_reset();
                    self.misses += 1;
                    return false;
                };
                if i == 0 {
                    *if whole {
                        &mut self.whole
                    } else {
                        &mut self.changes
                    } += 1;
                }
                canvases.push(canvas);
            }
            self.held = Some((version, s, canvases));
            match self.ack {
                Ack::Now => game.slide_applied(version),
                Ack::Twice => {
                    game.slide_applied(version);
                    game.slide_applied(version);
                }
                Ack::Never => {}
                Ack::Late => self.late.push(version),
            }
            true
        }

        /// The HD frame (from its export) with the shown map poisoned and the layers held drawn
        /// over it where `placed` (layer, x, y triples) puts them, clipped to the shown map.
        fn draw(&self, game: &Game, placed: &[i32]) -> Vec<u32> {
            let s = game.hd_scale() as usize;
            let fw = u6_render::WIDTH * s;
            let mut out = words_of(exported(game.hd_frame_ptr(), game.hd_frame_len()));
            let m = shown_map();
            let (mx, my, mw, mh) = (
                m[0] as usize * s,
                m[1] as usize * s,
                m[2] as usize * s,
                m[3] as usize * s,
            );
            for y in my..my + mh {
                out[y * fw + mx..y * fw + mx + mw].fill(POISON);
            }
            let canvases = self.held.as_ref().map_or(&[][..], |h| &h.2[..]);
            for p in placed.chunks_exact(3) {
                let (w, h, words) = &canvases[p[0] as usize];
                let (px, py) = (p[1] as i64 * s as i64, p[2] as i64 * s as i64);
                for r in 0..*h {
                    let ty = py + r as i64;
                    if !(my as i64..(my + mh) as i64).contains(&ty) {
                        continue;
                    }
                    for c in 0..*w {
                        let tx = px + c as i64;
                        let word = words[r * w + c];
                        if word >> 24 != 0 && (mx as i64..(mx + mw) as i64).contains(&tx) {
                            out[ty as usize * fw + tx as usize] = word;
                        }
                    }
                }
            }
            out
        }

        /// The canvases it holds against the slide's layers made last composed whole, here
        /// apart from the exports; panics on a difference.
        fn check(&self, game: &mut Game, label: &str) {
            let slide = game.layers.slide.clone().quiet();
            let engine = game.engine.as_ref().quiet();
            let held = self.held.as_ref().quiet();
            assert_eq!(
                held.0,
                game.slide_layers_version(),
                "{label}: the version made last"
            );
            assert_eq!(held.2.len(), slide.parts.len(), "{label}: every layer");
            for (i, part) in slide.parts.iter().enumerate() {
                let mut want = Vec::new();
                game.hd.compose_layer(engine, &part.pixels, &mut want);
                assert!(held.2[i].2 == want, "{label}: layer {i} as made whole");
            }
        }
    }

    /// The page's map words of `frame` (the shown map), and whether any is the poison.
    fn map_words(game: &Game, frame: &[u32]) -> (Vec<u32>, bool) {
        let s = game.hd_scale() as usize;
        let fw = u6_render::WIDTH * s;
        let m = shown_map();
        let (mx, my, mw, mh) = (
            m[0] as usize * s,
            m[1] as usize * s,
            m[2] as usize * s,
            m[3] as usize * s,
        );
        let words: Vec<u32> = (my..my + mh)
            .flat_map(|y| frame[y * fw + mx..y * fw + mx + mw].iter().copied())
            .collect();
        let poisoned = words.contains(&POISON);
        (words, poisoned)
    }

    /// Spec 20 Design 5's synthetic map for the page's tests: the view round world cell `centre`
    /// at the map's corner, each cell's base record 1 or 2 by its world cell (so it moves with the
    /// camera), and `movers` (slot, world cell) drawn with record 3; the leader slot 1.
    fn grid_scene(centre: (i32, i32), movers: &[(u16, (i32, i32))]) -> Scene {
        use u6_core::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, VIEW_H, VIEW_W};
        use u6_render::motion::{Cell, Node, Placed, Rank};
        let origin = (centre.0 - HALF_W as i32, centre.1 - HALF_H as i32);
        let mut cells: Vec<Cell> = (0..VIEW_W * VIEW_H)
            .map(|i| {
                let (x, y) = (
                    origin.0 + (i % VIEW_W) as i32,
                    origin.1 + (i / VIEW_W) as i32,
                );
                Cell {
                    base: 1 + u16::from((x * 3 + y * 5).rem_euclid(7) == 0),
                    nodes: Vec::new(),
                }
            })
            .collect();
        let mut placed = Vec::new();
        for &(slot, (x, y)) in movers {
            let (c, r) = ((x - origin.0) as usize, (y - origin.1) as usize);
            cells[r * VIEW_W + c].nodes.push(Node {
                word: 3,
                slot: Some(slot),
                rank: Rank::Own,
            });
            placed.push(Placed {
                slot,
                cell: (x, y),
                kind: 0x180,
            });
        }
        placed.sort_by_key(|p| p.slot);
        Scene {
            leader: Some(1),
            centre: (centre.0, centre.1, 0),
            origin,
            cols: VIEW_W,
            rows: VIEW_H,
            corner: (i32::from(MAP_X), i32::from(MAP_Y)),
            cells,
            placed,
            snap: false,
        }
    }

    /// Scene `n` of the synthetic walk: the leader a cell east each scene, an NPC (slot 2) a cell
    /// north or south by turns three cells west of it.
    fn walk_scene(n: i32) -> Scene {
        let at = 40 + n;
        grid_scene((at, 40), &[(1, (at, 40)), (2, (at - 3, 40 + n % 2))])
    }

    /// The engine of the page's tests: records 1..=3 of other pixels each, opaque, a palette of
    /// distinct colours presented once; then at each `w` the walk's next scene ([`walk_scene`]
    /// 1, 2, …) presented as a framed render keeps its scene (a pixel of the shown map set for
    /// it, so its picture is its own; [`u6_core::gfx::motion::keep_scene`]), at each `s` the same
    /// with a scene that snaps (as a load's first picture), at each `p` the palette's colours
    /// changed and presented (the shown map's colour indices as they were), and at each `f` the
    /// picture changed in place: another pixel of the shown map set and presented, kept with the
    /// scene of the same places (as a fade step's refresh).
    fn walk_engine() -> Engine {
        let mut state = u6_core::State::empty();
        for r in 1..=3u32 {
            let rec = &mut state.gfx.records[r as usize];
            rec.pixels = (0..256u32)
                .map(|i| ((i / 16 * r + i % 16 + r) % 5) as u8 + 1)
                .collect();
            rec.transparent = vec![false; 256];
        }
        let mut engine = Engine::with_game(state, Files::default(), |_state, io| async move {
            let palette = |fb: &mut Framebuffer, k: u8| {
                for (i, c) in fb.palette.iter_mut().enumerate() {
                    let v = i as u8;
                    *c = u6_core::Rgb::new(v.wrapping_mul(40) ^ k, v.wrapping_mul(90), 255 - v);
                }
            };
            io.draw(|fb| palette(fb, 0));
            io.present();
            let (x0, y0, _, _) = u6_core::geom::SHOWN;
            let (mut n, mut k, mut m) = (0, 0, 0);
            loop {
                while io.key_ready() {
                    match io.next_key().await {
                        Key::Char(c @ ('w' | 's')) => {
                            n += 1;
                            io.draw(|fb| fb.set(usize::from(x0) + n as usize, usize::from(y0), 7));
                            io.present();
                            let scene = Scene {
                                snap: c == 's',
                                ..walk_scene(n)
                            };
                            u6_core::gfx::motion::keep_scene(&io, scene);
                        }
                        Key::Char('p') => {
                            k += 0x55;
                            io.draw(|fb| palette(fb, k));
                            io.present();
                        }
                        Key::Char('f') => {
                            m += 1;
                            io.draw(|fb| fb.set(usize::from(x0) + m, usize::from(y0) + 1, 9));
                            io.present();
                            u6_core::gfx::motion::keep_scene(&io, walk_scene(n));
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

    /// The synthetic game for the page's tests: smooth motion on, then [`walk_engine`] on screen
    /// (which takes the switch from the page), HD on at 2×, the walk's first scene published and
    /// presented.
    fn walk_game() -> Game {
        let mut game = Game::new();
        game.set_motion(true);
        game.put_on_screen(walk_engine());
        game.set_hd(true, 2);
        publish_walk(&mut game);
        game.step(pacer::STEP_MS);
        game.present();
        game
    }

    /// The walk's next scene: `w` pressed, published at the next step.
    fn publish_walk(game: &mut Game) {
        game.key_down("KeyW", "w", false, 0.0);
    }

    /// A display frame of the page: a step of the engine, the present, the page's take; the hook
    /// `between` runs after the present and before the take.
    fn take_frame(
        game: &mut Game,
        page: &mut Page,
        between: &mut dyn FnMut(&mut Game, &mut Page),
    ) -> bool {
        game.step(pacer::STEP_MS);
        game.present();
        between(game, page);
        page.take(game)
    }

    /// What a case of the page's tests saw: the takes' checks passed, and per frame whether a
    /// slide was drawn while the presenter held one; per scene (from scene 1) the frames that
    /// drew a slide and the layers versions taken.
    struct Seen {
        frames: usize,
        drawn: usize,
        missed: usize,
        drawn_in: Vec<usize>,
        taken_in: Vec<usize>,
    }

    /// Runs the synthetic walk's scenes 1..=`scenes` on `page`, each slid to its end, `act` called
    /// with the scene's number and the frame's within it after each present; every new take
    /// checked against the layers made whole, and the drawn map free of poison.
    fn run_walk(
        game: &mut Game,
        page: &mut Page,
        scenes: i32,
        act: &mut dyn FnMut(i32, usize, &mut Game, &mut Page),
        label: &str,
    ) -> Seen {
        let mut seen = Seen {
            frames: 0,
            drawn: 0,
            missed: 0,
            drawn_in: Vec::new(),
            taken_in: Vec::new(),
        };
        let frames = (u6_render::motion::SLIDE_MS / f64::from(pacer::STEP_MS)).ceil() as usize + 2;
        for n in 1..=scenes {
            publish_walk(game);
            seen.drawn_in.push(0);
            seen.taken_in.push(0);
            for f in 0..frames {
                let taken = page.held.as_ref().map(|h| h.0);
                let drawn = take_frame(game, page, &mut |g: &mut Game, p: &mut Page| {
                    let before = (g.slide_layers_version(), g.slide_version());
                    act(n, f, g, p);
                    // New layers at the same places (made in the case's present) are a new slide
                    // for the page to draw.
                    assert!(
                        g.slide_layers_version() == before.0 || g.slide_version() != before.1,
                        "{label} scene {n} frame {f}: the slide's version moved with its layers"
                    );
                });
                seen.frames += 1;
                seen.drawn += usize::from(drawn);
                seen.missed += usize::from(!drawn && game.hd_on() && !game.slide().is_empty());
                if drawn {
                    *seen.drawn_in.last_mut().quiet() += 1;
                    if page.held.as_ref().map(|h| h.0) != taken {
                        page.check(game, &format!("{label} scene {n} frame {f}"));
                        *seen.taken_in.last_mut().quiet() += 1;
                    }
                    let frame = page.draw(game, &game.slide());
                    assert!(
                        !map_words(game, &frame).1,
                        "{label} scene {n} frame {f}: the map covered"
                    );
                }
            }
        }
        seen
    }

    /// Spec 20 Design 5 on synthetic scenes (no game files; the engine's renders keep them as its
    /// own, [`walk_engine`]): the page's side done through the exports as site/app.js does it
    /// ([`Page`]) takes each layers version whole once and then as changes (moved with the
    /// camera), each equal to the layers made whole, the shown map covered every frame of a slide;
    /// and it recovers, with the layers equal again, from a lost, a late and a repeated report
    /// (neither old one moving the exported version or base), from a reset followed by a report
    /// of layers made before it, from the switch off and on (the slides after it drawn and taken),
    /// from a pack set, from HD off and on at the same scale, from a palette change, and from the
    /// HD scale changed and HD off and on, each mid-slide; a camera restart 1.5 cells off covers
    /// the shown map. The cases' safeguards are named where they are checked.
    #[test]
    fn the_page_takes_the_slide_layers_through_the_exports_and_recovers_from_every_break() {
        let none = &mut |_: i32, _: usize, _: &mut Game, _: &mut Page| {};
        // The happy path: whole once, then changes; the slide drawn each frame of a slide.
        let (mut game, mut page) = (walk_game(), Page::new());
        let seen = run_walk(&mut game, &mut page, 6, none, "happy");
        assert_eq!((page.whole, page.misses), (1, 0), "happy: whole once");
        assert!(
            page.changes >= 5 && seen.drawn > 0 && seen.missed == 0,
            "happy: changes taken"
        );
        // Lost: scene 2's report never sent. The next changes are of the layers the game holds
        // as the page's (base) and the page holds others: it asks for them whole (safeguard: the
        // page's base check) and the layers are equal again.
        let lost = |page: &mut Page| {
            let mut game = walk_game();
            run_walk(
                &mut game,
                page,
                6,
                &mut |n, f, _, p: &mut Page| {
                    p.ack = if n == 2 && f == 0 {
                        Ack::Never
                    } else {
                        Ack::Now
                    };
                },
                "lost",
            );
        };
        let mut page = Page::new();
        lost(&mut page);
        assert_eq!(
            (page.misses, page.whole),
            (1, 2),
            "lost: one miss, whole again"
        );
        // Late: scene 2's report sent after scene 3's layers were made (made over scene 1's):
        // not taken as theirs (slide_applied matches the version made last), and the page asks
        // for scene 3's whole (safeguard: the page's base check).
        let late = |page: &mut Page| {
            let mut game = walk_game();
            run_walk(
                &mut game,
                page,
                6,
                &mut |n, f, g: &mut Game, p: &mut Page| {
                    p.ack = if n == 2 && f == 0 {
                        Ack::Late
                    } else {
                        Ack::Now
                    };
                    if n == 3 && f == 0 {
                        let exports = (g.slide_layers_version(), g.slide_layers_base());
                        for v in std::mem::take(&mut p.late) {
                            g.slide_applied(v);
                        }
                        assert_eq!(
                            (g.slide_layers_version(), g.slide_layers_base()),
                            exports,
                            "late: the old report changes neither the version nor the base"
                        );
                    }
                },
                "late",
            );
        };
        let mut page = Page::new();
        late(&mut page);
        assert_eq!(
            (page.misses, page.whole),
            (1, 2),
            "late: one miss, whole again"
        );
        // Without the page's base check both apply changes to layers they were not made from.
        for (name, case) in [("lost", &lost as &dyn Fn(&mut Page)), ("late", &late)] {
            let broken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut page = Page::new();
                page.check_base = false;
                case(&mut page);
            }));
            assert!(
                broken.is_err(),
                "{name} without the base check: other layers than made"
            );
        }
        // Twice: every report sent twice, and each sent again after the next layers were made
        // (before the page took them): neither takes anything (safeguard: slide_applied takes
        // the layers made last only, and once), so no layers are made whole again.
        let (mut game, mut page) = (walk_game(), Page::new());
        page.ack = Ack::Twice;
        run_walk(
            &mut game,
            &mut page,
            6,
            &mut |_, f, g: &mut Game, p: &mut Page| {
                if let (0, Some(held)) = (f, &p.held) {
                    let exports = (g.slide_layers_version(), g.slide_layers_base());
                    g.slide_applied(held.0);
                    assert_eq!(
                        (g.slide_layers_version(), g.slide_layers_base()),
                        exports,
                        "twice: the report again changes neither the version nor the base"
                    );
                }
            },
            "twice",
        );
        assert_eq!((page.misses, page.whole), (0, 1), "twice: whole once");
        // A reset (the page lost its canvases) then scene 2's report sent late, then the next
        // present: the report counts for nothing (safeguard: slide_reset drops the pending
        // layers), the layers come whole, and the page draws them with no miss.
        let (mut game, mut page) = (walk_game(), Page::new());
        run_walk(
            &mut game,
            &mut page,
            5,
            &mut |n, f, g: &mut Game, p: &mut Page| {
                p.ack = if n == 2 && f == 0 {
                    Ack::Late
                } else {
                    Ack::Now
                };
                if n == 2 && f == 3 {
                    p.held = None;
                    g.slide_reset();
                    for v in std::mem::take(&mut p.late) {
                        g.slide_applied(v);
                    }
                    g.present();
                }
            },
            "reset",
        );
        assert_eq!(
            (page.misses, page.whole),
            (0, 2),
            "reset: whole again, no miss"
        );
        // The switch off half way through scene 2's slide: nothing drawn from the next present
        // (safeguard: the switch clears the presenter and the engine's queue); on again, scene 3
        // starts the presenter afresh and scenes 4 to 6 slide, the page taking their layers
        // (safeguard: the switch on passes to the engine, whose renders keep their scenes again).
        let (mut game, mut page) = (walk_game(), Page::new());
        let mut off = 0;
        let seen = run_walk(
            &mut game,
            &mut page,
            6,
            &mut |n, f, g: &mut Game, _: &mut Page| {
                if n == 2 && f == 5 {
                    g.set_motion(false);
                    g.present();
                    off += usize::from(g.slide().is_empty() && !g.sliding());
                    g.set_motion(true);
                }
            },
            "switch",
        );
        assert_eq!(off, 1, "switch: off, no slide");
        assert_eq!(page.misses, 0, "switch: no miss");
        assert!(
            seen.drawn_in[3..].iter().all(|&d| d > 0) && seen.taken_in[3..].iter().all(|&t| t > 0),
            "switch: on again, scenes 4 to 6 slide and their layers are taken: drawn {:?}, taken {:?}",
            seen.drawn_in,
            seen.taken_in
        );
        // A pack set half way through scene 2: the next layers made whole in it (safeguard:
        // changes are only of layers made under the same HD view version, which the pack moves),
        // other than the layers before it, and equal to those made whole with it.
        let (mut game, mut page) = (walk_game(), Page::new());
        let (pack, _) = loaded(&[
            pack_tile(1, key_of(&game, 1), 7),
            pack_tile(3, key_of(&game, 3), 5),
        ]);
        let mut before = None;
        run_walk(
            &mut game,
            &mut page,
            4,
            &mut |n, f, g: &mut Game, p: &mut Page| match (n, f) {
                (2, 5) => {
                    before = Some((p.whole, p.held.as_ref().quiet().2[0].2.clone()));
                    g.set_hd_pack(&pack);
                    g.present();
                }
                (2, 6) => {
                    let (whole, statics) = before.as_ref().quiet();
                    assert_eq!(p.whole, whole + 1, "pack: the first take after it whole");
                    let now = &p.held.as_ref().quiet().2[0].2;
                    assert!(now != statics, "pack: the static layer changed with it");
                }
                _ => {}
            },
            "pack",
        );
        assert!(before.is_some(), "pack: set");
        assert_eq!((page.whole, page.misses), (2, 0), "pack: whole again");
        // A scene that snaps (as a load's first picture) after scene 2's slide: the next layers
        // come whole (safeguard: a scene that snaps drops the layers the page holds as the base
        // of changes, whose sources a load names again).
        let (mut game, mut page) = (walk_game(), Page::new());
        let mut at_snap = None;
        run_walk(
            &mut game,
            &mut page,
            4,
            &mut |n, f, g: &mut Game, p: &mut Page| {
                if (n, f) == (2, 12) {
                    at_snap = Some(p.whole);
                    g.key_down("KeyS", "s", false, 0.0);
                }
            },
            "snap",
        );
        assert_eq!(at_snap, Some(1), "snap: whole once before it");
        assert_eq!(
            (page.whole, page.misses),
            (2, 0),
            "snap: whole again after it"
        );
        // HD off and on again at the same scale half way through scene 2: the page keeps its
        // canvases, the layers come whole (made under the new HD view version) and equal to those
        // made fresh, the versions moved.
        let (mut game, mut page) = (walk_game(), Page::new());
        let mut toggled = None;
        run_walk(
            &mut game,
            &mut page,
            4,
            &mut |n, f, g: &mut Game, p: &mut Page| match (n, f) {
                (2, 5) => {
                    toggled = Some((p.whole, g.slide_layers_version(), g.slide_version()));
                    g.set_hd(false, 2);
                    g.present();
                    g.set_hd(true, 2);
                    g.present();
                }
                (2, 6) => {
                    let (whole, layers, slide) = toggled.quiet();
                    assert_eq!(p.whole, whole + 1, "HD off and on: taken whole");
                    assert!(
                        g.slide_layers_version() > layers && g.slide_version() > slide,
                        "HD off and on: the versions moved"
                    );
                }
                _ => {}
            },
            "toggle",
        );
        assert_eq!(
            (page.whole, page.misses),
            (2, 0),
            "toggle: whole again, no miss"
        );
        // The palette changed between scenes 2 and 3 (the same colour indices): scene 3's layers
        // come whole in the new colours (safeguard: changes are only of layers composed in the
        // same palette), equal to those made fresh.
        let (mut game, mut page) = (walk_game(), Page::new());
        run_walk(
            &mut game,
            &mut page,
            4,
            &mut |n, f, g: &mut Game, _: &mut Page| {
                if (n, f) == (2, 12) {
                    g.key_down("KeyP", "p", false, 0.0);
                }
            },
            "palette",
        );
        assert_eq!(
            (page.whole, page.misses),
            (2, 0),
            "palette: whole again, no miss"
        );
        // A camera restart: scene 3 published half way through scene 2's slide, so the camera
        // restarts from where it is drawn, 1.5 cells from its new place (the most a restart takes,
        // past it the camera snaps); every frame of it covers the shown map (the static layer's
        // cell round the view and the shown map's half cell inside the view: the bound
        // `u6_core::gfx::motion` asserts).
        let (mut game, mut page) = (walk_game(), Page::new());
        let mut restarted = 0;
        run_walk(
            &mut game,
            &mut page,
            4,
            &mut |n, f, g: &mut Game, _: &mut Page| {
                if (n, f) == (2, 8) {
                    publish_walk(g);
                }
                if (n, f) == (2, 9) {
                    let camera = g.motion.presenter.view(g.presentation_ms()).quiet().camera;
                    restarted = camera.0.abs();
                }
            },
            "restart",
        );
        assert!(
            restarted > 16,
            "restart: the camera drawn more than a cell off ({restarted} px)"
        );
        assert_eq!(page.misses, 0, "restart: no miss");
        // HD 4× half way through scene 2, HD off through scene 3, 2× again from scene 4: the
        // layers made again at each (safeguard: the layers are made for the HD view's version)
        // and taken whole at the new scale (the page holds none at it).
        let (mut game, mut page) = (walk_game(), Page::new());
        run_walk(
            &mut game,
            &mut page,
            6,
            &mut |n, f, g: &mut Game, _: &mut Page| {
                match (n, f) {
                    (2, 5) => g.set_hd(true, 4),
                    (3, 0) => g.set_hd(false, 4),
                    (4, 0) => g.set_hd(true, 2),
                    _ => return,
                }
                g.present();
            },
            "scale",
        );
        assert_eq!(
            (page.whole, page.misses),
            (3, 0),
            "scale: whole at each scale"
        );
    }

    /// Spec 20 Design 5 on the user's zip (skipped when absent): with HD on at 2× and smooth motion
    /// on, steps in the start fight, the page's side done through the exports ([`Page`]); each
    /// layers version drawn at rest (the camera and every mover at its place) over the HD frame is
    /// the HD frame's map, word for word, with no poison left; the slide's frames cover the shown
    /// map; and a pack set between two slides (records of the shown map) changes the HD frame's
    /// map, the first layers taken after it come whole, and drawn at rest they equal the HD frame
    /// in it.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn the_hd_slide_layers_and_their_changes_draw_the_hd_frame_at_rest() {
        if !u6_testkit::archive().exists() {
            eprintln!("skipped: the local archive not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
        let mut game = Game::new();
        game.collect("ultima-vi.zip", data).quiet();
        game.set_motion(true);
        // Exact original-frame oracle; default B has separate posed-pixel regressions.
        game.set_footstep_pattern(0);
        // This receipt isolates W2's unposed layer contract. Physical attacks have their
        // own complete classic/HD raster oracle (attacks_from_the_real_command...).
        game.set_attack_suspended(true);
        game.set_hd(true, 2);
        game.boot(3, true).quiet();
        let mut page = Page::new();
        let (mut versions, mut checked, mut covered) = (0, [0, 0], 0);
        let dt = 1000.0 / 60.0;
        let mut keys = 0;
        let mut packed = None;
        let (mut first_after_pack, mut pack_words) = (true, 0);
        for i in 0..30_000 {
            if keys < 8 && game.at_prompt() && i > 3000 {
                if keys == 4 && packed.is_none() {
                    // Between two slides: a pack of the records the shown map draws most.
                    let mut counts = BTreeMap::new();
                    {
                        let e = game.engine.as_ref().quiet();
                        let f = e.published_frame();
                        let (x0, y0, x1, y1) = u6_core::geom::SHOWN;
                        for y in usize::from(y0)..=usize::from(y1) {
                            for x in usize::from(x0)..=usize::from(x1) {
                                let src = f.src_at(x, y).filter(|s| s.kind == u6_core::Kind::Tile);
                                if let Some(src) = src {
                                    *counts.entry(src.id).or_insert(0usize) += 1;
                                }
                            }
                        }
                    }
                    let mut most: Vec<(usize, u16)> =
                        counts.into_iter().map(|(r, n)| (n, r)).collect();
                    most.sort_unstable_by(|a, b| b.cmp(a));
                    let images: Vec<_> = (most.iter().take(3).enumerate())
                        .map(|(k, &(_, r))| pack_tile(r, key_of(&game, r), 3 + k as u8))
                        .collect();
                    let (pack, _) = loaded(&images);
                    let (map, _) = map_words(&game, game.hd.rgba());
                    game.set_hd_pack(&pack);
                    let (now, _) = map_words(&game, game.hd.rgba());
                    let changed = map.iter().zip(&now).filter(|(a, b)| a != b).count();
                    assert!(changed > 0, "the pack changes the HD frame's map");
                    packed = Some((page.whole, changed));
                }
                game.key_down("ArrowRight", "ArrowRight", false, 0.0);
                keys += 1;
            }
            let k = game.pace(dt);
            for _ in 0..k {
                game.step(pacer::STEP_MS);
            }
            game.ran(k);
            if k == 0 && !game.sliding() {
                continue;
            }
            game.present();
            let taken = page.held.as_ref().map(|h| h.0);
            if !page.take(&mut game) {
                continue;
            }
            let (_, poisoned) = map_words(&game, &page.draw(&game, &game.slide()));
            assert!(!poisoned, "the slide's frame covers the shown map");
            covered += 1;
            if page.held.as_ref().map(|h| h.0) == taken {
                continue;
            }
            versions += 1;
            page.check(&mut game, &format!("version {versions}"));
            // The first layers taken after the pack: whole, in it.
            if let Some((whole, changed)) = packed.filter(|_| first_after_pack) {
                assert_eq!(page.whole, whole + 1, "the first take after the pack whole");
                first_after_pack = false;
                pack_words = changed;
            }
            // At rest over the HD frame, when the newest scene is the engine's view now.
            let slide = game.layers.slide.as_ref().quiet();
            let rest = View {
                camera: (0, 0),
                movers: Vec::new(),
            };
            let placed: Vec<i32> = u6_core::gfx::motion::slide_placements(slide, &rest)
                .into_iter()
                .flat_map(|(l, x, y)| [l as i32, x, y])
                .collect();
            if game.motion.presenter.history().first().is_some_and(|n| {
                let e = game.engine.as_ref().quiet();
                e.state().map.centre.x as i32 == n.scene.centre.0
                    && e.state().map.centre.y as i32 == n.scene.centre.1
            }) {
                let (words, poisoned) = map_words(&game, &page.draw(&game, &placed));
                let (want, _) = map_words(&game, game.hd.rgba());
                assert!(
                    !poisoned && words == want,
                    "version {versions}: at rest, the HD frame's map"
                );
                checked[usize::from(packed.is_some())] += 1;
            }
        }
        assert!(
            keys == 8 && checked[0] > 0 && checked[1] > 0 && page.changes > 0,
            "versions checked before and after the pack, and changes taken: {checked:?}"
        );
        assert!(!first_after_pack, "layers taken after the pack");
        println!(
            "{versions} layers versions taken ({} whole, {} as changes, {} misses); {} drawn at \
             rest equal to the HD frame before the pack and {} after (the pack changed \
             {pack_words} words of the HD frame's map, the first take after it whole); {covered} \
             frames with the map covered",
            page.whole, page.changes, page.misses, checked[0], checked[1]
        );
    }

    /// Spec 20 Design 5, the page's side at a slide's end (the product code; no game files, HD
    /// on at 2×): halfway through a step's slide the engine's picture changes in place (`f`, as a
    /// fade step's refresh), and the slide runs on with its layers, made from the step's picture
    /// (B). At the first present whose presentation clock reaches the step's publication plus
    /// [`u6_render::motion::SLIDE_MS`], no layer is placed ([`Game::slide`] empty: the page shows
    /// its HD frame alone), and the HD frame is the engine's picture now (B'), not B.
    #[test]
    fn at_a_slides_end_the_page_shows_the_engines_hd_frame() {
        let mut game = walk_game();
        let frame = |game: &mut Game| {
            game.step(pacer::STEP_MS);
            game.present();
        };
        for _ in 0..60 {
            frame(&mut game);
        }
        assert!(!game.sliding(), "the first scene settled");
        publish_walk(&mut game);
        game.step(pacer::STEP_MS);
        // The step's publication's time: the engine's at the end of its tick
        // ([`u6_core::engine::Published::time_ms`]).
        let published = game.engine.as_ref().quiet().io().now_ms() as f64;
        game.present();
        assert!(!game.slide().is_empty(), "the step slides");
        let b = game.hd_frame_rgba();
        let made = game.slide_layers_version();
        while game.presentation_ms() < published + u6_render::motion::SLIDE_MS / 2.0 {
            frame(&mut game);
        }
        game.key_down("KeyF", "f", false, 0.0);
        frame(&mut game);
        let b2 = game.hd_frame_rgba();
        assert!(
            !game.slide().is_empty() && game.slide_layers_version() == made,
            "in place: the slide runs on with B's layers"
        );
        assert!(
            b2 != b && b2 == expected_hd(&game, Scale::X2),
            "the HD frame composed again: B'"
        );
        let end = published + u6_render::motion::SLIDE_MS;
        let mut frames = 0;
        loop {
            game.step(pacer::STEP_MS);
            let at_end = game.presentation_ms() >= end;
            game.present();
            frames += 1;
            if at_end {
                break;
            }
            assert!(!game.slide().is_empty(), "before its end the slide placed");
        }
        assert!(
            game.slide().is_empty(),
            "at the slide's end no layer placed: the page shows its HD frame"
        );
        let now = game.hd_frame_rgba();
        assert!(
            now == expected_hd(&game, Scale::X2) && now == b2 && now != b,
            "the HD frame the engine's picture now: B', not B"
        );
        println!(
            "a step's slide with an in-place change half way: no layer placed at the slide's end \
             (the {frames}th present after the change, the first at it); the HD frame B'"
        );
    }

    /// Spec 20 Bounds, on the user's zip, in this build's layout: the bytes of the slide's layers
    /// at the page's largest scale (HD 4×) over four taps east, each a move of the party: the
    /// wasm side's layers (colour indices, sources and drawn marks; each layer the slide and the
    /// kept ones share once), their composed words and changes, and the canvases of the test's
    /// stand-in for the page ([`Page`]: the layers' canvases only, not the page's screen canvas
    /// nor Wide's compositing canvas). Each the most over the frames from the first tap on that
    /// drew a move's slide while it ran (every move has some), and the most of their sum on one
    /// frame.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn the_slide_layer_buffers_at_hd_4x() {
        if !u6_testkit::archive().exists() {
            assert_ne!(
                std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
                Ok("1"),
                "U6_REQUIRE_ARCHIVE=1 and the local archive is not present"
            );
            eprintln!("skipped: the local archive not present");
            return;
        }
        let data = std::fs::read(u6_testkit::archive()).quiet();
        let mut game = Game::new();
        game.collect("ultima-vi.zip", data).quiet();
        game.set_motion(true);
        game.set_hd(true, 4);
        game.boot(3, true).quiet();
        assert_eq!(game.hd_scale(), 4, "the largest scale");
        let mut page = Page::new();
        let dt = 1000.0 / 60.0;
        let centre = |game: &Game| {
            let e = game.engine.as_ref().quiet();
            (e.state().map.centre.x, e.state().map.centre.y)
        };
        let (mut moves, mut frames, mut at) = (0, 0, centre(&game));
        // From the first press on: per move (1 to 4), the frames sampled while its slide ran.
        let (mut pressed, mut per_move) = (false, [0usize; 5]);
        let (mut pixels, mut words, mut canvases, mut together) = (0usize, 0usize, 0usize, 0usize);
        let bytes = |l: &u6_render::motion::LayerPixels| {
            l.pixels.capacity()
                + l.src.capacity() * std::mem::size_of::<u6_render::Src>()
                + l.drawn.capacity() * std::mem::size_of::<bool>()
        };
        for i in 0..30_000 {
            // The party's moves after the first press (a cell east each).
            if centre(&game) != at {
                moves += usize::from(pressed);
                at = centre(&game);
            }
            let settled = game.at_prompt() && !game.sliding();
            if moves == 4 && settled {
                break;
            }
            // A tap: pressed and let go at once, so the held walk (spec 19 O1c) adds no press.
            if i > 3000 && settled && game.pending.is_empty() {
                let t = game.clock;
                game.key_down("ArrowRight", "ArrowRight", false, t);
                game.key_up("ArrowRight");
                pressed = true;
            }
            let k = game.pace(dt);
            for _ in 0..k {
                game.step(pacer::STEP_MS);
            }
            game.ran(k);
            if k == 0 && !game.sliding() {
                continue;
            }
            game.present();
            let running = game.motion.presenter.active(game.presentation_ms());
            if !page.take(&mut game) || !running || game.layers.slide.is_none() || moves == 0 {
                continue;
            }
            frames += 1;
            per_move[moves.min(4)] += 1;
            let l = &game.layers;
            let mut held: Vec<&Rc<u6_render::motion::LayerPixels>> = Vec::new();
            for part in l.slide.iter().flat_map(|s| &s.parts) {
                held.push(&part.pixels);
            }
            for kept in [&l.held, &l.pending].into_iter().flatten() {
                held.extend([&kept.statics, &kept.frame]);
            }
            let mut once: Vec<&Rc<u6_render::motion::LayerPixels>> = Vec::new();
            for p in held {
                if !once.iter().any(|o| Rc::ptr_eq(o, p)) {
                    once.push(p);
                }
            }
            let these = [
                once.iter().map(|p| bytes(p)).sum::<usize>(),
                (l.images.iter())
                    .map(|im| {
                        let patch = im.patch.as_ref().map_or(0, |p| {
                            (p.rects.iter()).map(|r| r.1.capacity()).sum::<usize>()
                        });
                        4 * (im.rgba.capacity() + patch)
                    })
                    .sum(),
                (page.held.as_ref()).map_or(0, |h| h.2.iter().map(|c| 4 * c.2.len()).sum()),
            ];
            pixels = pixels.max(these[0]);
            words = words.max(these[1]);
            canvases = canvases.max(these[2]);
            together = together.max(these.iter().sum());
        }
        assert!(
            moves == 4 && per_move[1..].iter().all(|&n| n > 0),
            "four moves, each with frames drawn while its slide ran: {moves} moves, {per_move:?}"
        );
        println!(
            "the slide's layers at HD 4x over {moves} moves, the most over the {frames} frames \
             drawn while a slide ran: {pixels} bytes of layers, {words} of composed words and \
             changes, {canvases} of the stand-in page's layer canvases; the sum of these peaks \
             {}, the most on one frame {together}",
            pixels + words + canvases
        );
    }

    /// Spec 17: before a start there is no context menu, none drawn, and the game holds no button.
    #[test]
    fn before_a_start_there_is_no_context_menu() {
        let mut game = Game::new();
        let (x, y) = hero_point();
        assert_eq!(game.context_menu(x, y), "");
        game.pointer(x, y, 2);
        game.step(20);
        assert_eq!(game.menu_view(), "");
        assert_eq!(game.game_buttons(), 0);
        assert!(game.pending.is_empty());
        assert!(game.published_rgba().is_empty(), "no published frame");
    }

    /// Spec 17: the context menu's JSON: its kind, cell, underlined command and sections, each
    /// entry's code, whether it can run, its reason's code and the theft mark, and Go here; no
    /// names (the section's name never appears).
    #[test]
    fn the_context_menus_json() {
        use u6_core::ui::menu::{Entry, GO_HERE, Kind, Menu, Reason, Section};
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
            go_here: Some(entry(GO_HERE, Some(Reason::Unreachable), false)),
        };
        assert_eq!(
            menu_json(&map),
            r#"{"kind":"map","cell":[3,-1],"underlined":136,"sections":[{"object":300,"kind":[128,2],"entries":[{"code":136,"enabled":false,"reason":"far","theft":false},{"code":133,"enabled":true,"reason":null,"theft":true}]}],"go_here":{"code":256,"enabled":false,"reason":"unreachable","theft":false}}"#
        );
        let panel = Menu {
            kind: Kind::Panel,
            cell: None,
            underlined: None,
            sections: Vec::new(),
            go_here: None,
        };
        assert_eq!(
            menu_json(&panel),
            r#"{"kind":"panel","cell":null,"underlined":null,"sections":[],"go_here":null}"#
        );
    }

    /// Spec 17 Presentation: the drawn menu's JSON: the box and the text area as x, y, width,
    /// height, the highlighted row, whether it is in the Korean words (round 5, MUST 4), each
    /// row's kind, code, whether it runs, reason, theft mark and top pixel row.
    #[test]
    fn the_drawn_menus_json() {
        use u6_core::ui::menu::Reason;
        use u6_core::ui::popup::{RowKind, RowView, View};
        let row = |kind, code, reason, theft, y| RowView {
            kind,
            code,
            reason,
            theft,
            y,
        };
        let v = View {
            rect: (35, 51, 100, 84),
            text: (40, 56, 95, 79),
            hover: 1,
            korean: false,
            rows: vec![
                row(RowKind::Head, None, None, false, 56),
                row(RowKind::Entry, Some(0x85), None, true, 64),
                row(RowKind::Note, None, None, false, 72),
            ],
        };
        assert_eq!(
            view_json(&v, [0x10_2030, 0x40_5060, 0xff_0001]),
            r#"{"rect":[35,51,66,34],"text":[40,56,56,24],"hover":1,"korean":false,"colours":[1056816,4214880,16711681],"rows":[{"kind":"head","code":null,"enabled":false,"reason":null,"theft":false,"y":56},{"kind":"entry","code":133,"enabled":true,"reason":null,"theft":true,"y":64},{"kind":"note","code":null,"enabled":false,"reason":null,"theft":false,"y":72}]}"#
        );
        let off = View {
            rows: vec![row(
                RowKind::Entry,
                Some(0x88),
                Some(Reason::Far),
                false,
                64,
            )],
            ..v
        };
        assert!(view_json(&off, [0; 3]).contains(r#""enabled":false,"reason":"far""#));
        let korean = View {
            korean: true,
            ..off
        };
        assert!(view_json(&korean, [0; 3]).contains(r#""hover":1,"korean":true,"colours""#));
    }

    /// Presses the pointer's `buttons` at (x, y) and lets them go, a step after each.
    fn click(game: &mut Game, (x, y): (i32, i32), buttons: u8) {
        game.pointer(x, y, buttons);
        game.step(20);
        game.pointer(x, y, 0);
        game.step(20);
    }

    /// The engine's open context menu, if one is drawn.
    fn drawn(game: &Game) -> Option<u6_core::ui::popup::View> {
        game.engine.as_ref()?.context_menu()
    }

    /// The frame pixel of row `i`'s label (the text area's second column, the row's middle).
    fn row_point(v: &u6_core::ui::popup::View, i: usize) -> (i32, i32) {
        (v.text.0 + 12, v.rows[i].y + 4)
    }

    /// The row of the drawn menu `v` with entry `code`.
    fn row_of(v: &u6_core::ui::popup::View, code: u16) -> Option<usize> {
        v.rows.iter().position(|r| r.code == Some(code))
    }

    /// Spec 17 Acceptance 8, on the user's zip (skipped when absent): at the prompt a right press
    /// on the hero ([`hero_point`], the view's centre cell, either build's layout) opens the menu
    /// in the frame: its box's pixels differ from the published frame's, every pixel outside it is
    /// the published frame's; the game took no button, also with the right button held and
    /// moved over the map; Look is offered and a left press on it runs it through its target read
    /// (Look then shows the portrait and waits for a key: no prompt), the menu closed. A key
    /// gets the prompt back.
    #[test]
    fn a_right_press_opens_the_drawn_menu_and_look_runs_through_the_target_read() {
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        let (hx, hy) = hero_point();
        let at = game.leader_position();
        let presses = game.game_presses();
        game.pointer(hx, hy, 2);
        game.step(20);
        let v = drawn(&game).expect("the menu open");
        let still = |game: &Game, what: &str| {
            assert_eq!(game.game_buttons(), 0, "{what}: the game holds no button");
            assert_eq!(
                game.game_presses(),
                presses,
                "{what}: no press reached the game"
            );
            assert_eq!(game.leader_position(), at, "{what}: no walk");
        };
        still(&game, "the opening step");
        // Held and moved over the map, the page pacing its steps as it does with a button held
        // (Game::pace reads it held, spec 06): each step the game holds no button, no walk.
        for (x, y) in [(hx + 20, hy), (hx + 20, hy + 20), (hx - 30, hy)] {
            game.pointer(x, y, 2);
            assert!(game.pointer_held(), "the page's button held");
            let k = game.pace(40.0).max(1);
            for _ in 0..k {
                game.step(20);
                still(&game, "a held move");
            }
            game.ran(k);
            // K3 merge note 7: the page's game pointer is the menu's while it takes the pointer.
            let gp = game.game_pointer();
            assert_eq!(
                (gp[0], gp[1], gp[2], gp[3]),
                (x, y, 1, 0),
                "game_pointer: the menu's, drawn, nothing queued"
            );
        }
        game.pointer(hx - 30, hy, 0);
        game.step(20);
        still(&game, "the release");
        assert_eq!(
            game.game_buttons(),
            0,
            "the held right button gave the game nothing"
        );
        assert!(
            game.at_prompt() && drawn(&game).is_some(),
            "open, the prompt up"
        );
        {
            let e = game.engine.as_ref().quiet();
            let (screen, published) = (e.screen_frame(), e.published_frame());
            let (x0, y0, x1, y1) = v.rect;
            let (mut inside, mut differ, mut outside) = (0, 0, 0);
            for y in 0..u6_render::HEIGHT {
                for x in 0..u6_render::WIDTH {
                    let (a, b) = (screen.get(x, y), published.get(x, y));
                    let (xi, yi) = (x as i32, y as i32);
                    if (x0..=x1).contains(&xi) && (y0..=y1).contains(&yi) {
                        inside += 1;
                        differ += usize::from(a != b);
                    } else {
                        outside += usize::from(a != b);
                    }
                }
            }
            println!("box pixels {inside}, differing {differ}; outside differing {outside}");
            assert!(differ * 2 > inside, "the menu drawn in its box");
            assert_eq!(outside, 0, "the frame outside the box as published");
        }
        game.present();
        let mut want = Vec::new();
        game.engine
            .as_ref()
            .quiet()
            .published_frame()
            .write_rgba(&mut want);
        assert_eq!(
            game.published_rgba(),
            want,
            "Game::published_rgba: the published frame"
        );
        // The menu in the frame only (round 2b, opus NIT 5): the frame differs from the published
        // one in most of the menu's box, and outside it only under the pointer's sprite.
        {
            let (frame, published) = (game.frame_rgba(), game.published_rgba());
            let sprite = game.engine.as_ref().quiet().sprite().cloned();
            let under_sprite = |x: i32, y: i32| {
                sprite.as_ref().is_some_and(|sp| {
                    let (sx, sy) = (i32::from(sp.x), i32::from(sp.y));
                    (sx..sx + 16).contains(&x) && (sy..sy + sp.rows.len() as i32).contains(&y)
                })
            };
            let (x0, y0, x1, y1) = v.rect;
            let (mut boxed, mut differ, mut outside) = (0, 0, 0);
            for y in 0..u6_render::HEIGHT as i32 {
                for x in 0..u6_render::WIDTH as i32 {
                    let i = (y as usize * u6_render::WIDTH + x as usize) * 4;
                    let d = frame[i..i + 3] != published[i..i + 3];
                    if (x0..=x1).contains(&x) && (y0..=y1).contains(&y) {
                        boxed += 1;
                        differ += usize::from(d);
                    } else if !under_sprite(x, y) {
                        outside += usize::from(d);
                    }
                }
            }
            println!(
                "RGBA frame: {differ} of {boxed} box pixels differ; outside, off the sprite, {outside}"
            );
            assert!(differ * 2 > boxed, "the menu in the RGBA frame's box");
            assert_eq!(outside, 0, "outside the box only the sprite differs");
        }
        let look = row_of(&v, 0x84).expect("Look offered");
        assert!(v.rows[look].enabled(), "Look can run");
        click(&mut game, row_point(&v, look), 1);
        assert!(drawn(&game).is_none(), "chosen: closed");
        for _ in 0..200 {
            game.step(20);
            if !game.at_prompt() {
                break;
            }
        }
        for _ in 0..50 {
            game.step(20);
        }
        assert!(
            !game.at_prompt(),
            "Look read its target and waits on the portrait"
        );
        {
            let e = game.engine.as_ref().quiet();
            let input = &e.state().ui.input;
            assert!(
                !e.context_menu_waits(),
                "the target press went in: no run held"
            );
            assert_eq!(e.io().pending_events(), 0, "and the game took it: consumed");
            assert!(input.in_wait, "the portrait's key wait");
        }
        game.key_down("Space", " ", false, 0.0);
        assert!(steps_to_prompt(&mut game), "the prompt again");
    }

    /// Spec 17 Acceptance 6, on the user's zip: Escape and a left press outside the box close the
    /// menu; neither the game's key nor its button sees them, no line is printed and the hero
    /// stays (no turn passed). The press outside is the menu's to its release.
    #[test]
    fn escape_and_a_press_outside_close_the_menu_and_give_the_game_nothing() {
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        let (hx, hy) = hero_point();
        let at = game.leader_position();
        let lines = |g: &Game| g.engine.as_ref().quiet().state().ui.messages.len();
        let before = lines(&game);
        click(&mut game, (hx, hy), 2);
        assert!(drawn(&game).is_some(), "open");
        assert!(game.key_down("Escape", "Escape", false, 0.0));
        game.step(20);
        assert!(game.key_repeat("Escape", "Escape", false, 0.0));
        game.step(20);
        assert!(game.key_up("Escape"));
        for _ in 0..30 {
            game.step(20);
        }
        assert!(drawn(&game).is_none(), "Escape closed it");
        assert!(game.at_prompt(), "the prompt still up");
        click(&mut game, (hx, hy), 2);
        let v = drawn(&game).expect("open again");
        let (sx0, sy0, ..) = u6_core::geom::SHOWN;
        let out = (i32::from(sx0) + 2, i32::from(sy0) + 2);
        assert!(
            !(v.rect.0..=v.rect.2).contains(&out.0) || !(v.rect.1..=v.rect.3).contains(&out.1),
            "the press point outside the box"
        );
        game.pointer(out.0, out.1, 1);
        game.step(20);
        assert_eq!(game.game_buttons(), 0, "the press outside: not the game's");
        game.pointer(out.0, out.1, 0);
        for _ in 0..30 {
            game.step(20);
        }
        assert!(drawn(&game).is_none(), "closed");
        assert_eq!(lines(&game), before, "no line printed");
        assert_eq!(game.leader_position(), at, "the hero where he was");
        assert!(game.at_prompt(), "the prompt up");
    }

    /// One display frame of the page's loop at its clock `t` (wall ms) after `dt`: the clock
    /// given, then the steps the pace gives (at most 40: Max's budget here), as `app.js` `frame`
    /// runs them.
    fn page_frame(game: &mut Game, t: f64, dt: f64) {
        game.set_clock(t);
        let k = game.pace(dt).min(40);
        for _ in 0..k {
            game.step(step_ms());
        }
        game.ran(k);
    }

    /// [`page_frame`]s 16 wall ms apart from `t` on until the page's queued events all reached
    /// the game: a frame with a press held runs at 1× (the pacer's rule), so the first after a
    /// press may run no step.
    fn settle(game: &mut Game, t: &mut f64) {
        for _ in 0..20 {
            *t += 16.0;
            page_frame(game, *t, 16.0);
            if game.pending.is_empty() {
                return;
            }
        }
        panic!("the queued events given within 20 frames");
    }

    /// Spec 17 Presentation, Input (round 2b, opus MUST 1), on the user's zip: at 8× and at Max
    /// a double click on Go here, its second press stepped about 200 page (wall) ms after the
    /// choosing one, is the menu's: no press reaches the game though the engine's clock ran on
    /// for far more than the 500 ms window meanwhile; Go here asked once. About 600 page ms
    /// after it, the press is the game's. Past the castle's fight ([`castle_game`], the menu on
    /// as the page puts it on): since round 4 Go here is the real travel's, refused while the
    /// fight is on; the cell is the first two cells from the hero whose Go here can run.
    #[test]
    fn a_double_click_at_speed_is_timed_on_the_pages_clock() {
        for speed in [8.0, 0.0] {
            for (gap, menus) in [(200.0, true), (600.0, false)] {
                let Some(mut game) = castle_game() else {
                    return;
                };
                game.engine.as_mut().quiet().set_context_menu(true);
                game.set_speed(speed);
                let ms = |g: &Game| g.engine.as_ref().quiet().io().now_ms();
                let (hx, hy) = hero_point();
                let at = [(32, 0), (-32, 0), (0, 32), (0, -32), (32, 32), (-32, -32)]
                    .into_iter()
                    .map(|(dx, dy)| (hx + dx, hy + dy))
                    .find(|&(x, y)| {
                        let e = game.engine.as_ref().quiet();
                        u6_core::ui::menu::query(&e.state(), e.io(), x as i16, y as i16)
                            .and_then(|m| m.go_here)
                            .is_some_and(|g| g.enabled())
                    })
                    .expect("a cell two from the hero whose Go here can run");
                let mut t = 1_000.0;
                page_frame(&mut game, t, 16.0);
                game.pointer(at.0, at.1, 2);
                settle(&mut game, &mut t);
                game.pointer(at.0, at.1, 0);
                settle(&mut game, &mut t);
                let v = drawn(&game).expect("the menu open");
                let go = row_of(&v, u6_core::ui::menu::GO_HERE).expect("Go here");
                assert!(v.rows[go].enabled(), "Go here can run");
                let (gx, gy) = row_point(&v, go);
                game.pointer(gx, gy, 1);
                settle(&mut game, &mut t);
                let (t0, e0) = (t, ms(&game));
                assert!(drawn(&game).is_none(), "chosen: closed");
                assert_eq!(game.go_here_asked(), 1, "Go here asked");
                game.pointer(gx, gy, 0);
                settle(&mut game, &mut t);
                while t + 16.0 < t0 + gap {
                    t += 16.0;
                    page_frame(&mut game, t, 16.0);
                }
                let presses = game.game_presses();
                game.pointer(gx, gy, 1);
                settle(&mut game, &mut t);
                let (wall, ran) = (t - t0, ms(&game) - e0);
                println!(
                    "speed {speed}: the second press {wall} page ms on, the engine's clock {ran} ms"
                );
                assert!(
                    (gap..gap + 64.0).contains(&wall),
                    "precondition: the gap as meant"
                );
                assert!(ran > 500, "the engine's clock past the window");
                assert_eq!(
                    game.game_presses() - presses,
                    f64::from(u8::from(!menus)),
                    "speed {speed}, {wall} page ms: the second press the menu's: {menus}"
                );
                game.pointer(gx, gy, 0);
                settle(&mut game, &mut t);
                assert_eq!(game.go_here_asked(), 1, "chosen once");
            }
        }
    }

    /// Spec 17 Presentation (round 2b, gpt e MUST), on the user's zip: a right press on the
    /// canvas and then a press off it (`Game::dismiss_menu`), both before one step: the
    /// dismissal waits its turn behind the press, so the step opens the menu and closes it,
    /// and it ends closed; nothing reaches the game. With nothing queued the dismissal acts at
    /// once.
    #[test]
    fn a_press_off_the_canvas_after_a_queued_right_press_leaves_the_menu_closed() {
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        let (hx, hy) = hero_point();
        let presses = game.game_presses();
        game.pointer(hx, hy, 2);
        assert!(!game.dismiss_menu(), "queued behind the right press");
        game.pointer(hx + 3, hy, 2);
        game.step(20);
        assert!(drawn(&game).is_none(), "the menu ends closed");
        assert_eq!(game.game_presses(), presses, "nothing reached the game");
        game.pointer(hx + 3, hy, 0);
        game.step(20);
        assert!(
            drawn(&game).is_none() && game.at_prompt(),
            "closed, the prompt up"
        );
        game.pointer(hx, hy, 2);
        game.step(20);
        game.pointer(hx, hy, 0);
        game.step(20);
        assert!(drawn(&game).is_some(), "opened by a right press alone");
        assert!(game.dismiss_menu(), "nothing queued: closed at once");
        assert!(drawn(&game).is_none(), "closed");
        assert_eq!(game.game_presses(), presses, "nothing reached the game");
    }

    /// Spec 17 Presentation (round 2b, gpt f SHOULD; round 3, gpt c SHOULD): `menu_hidden` reads
    /// the rects the engine holds, not the page's copy: none without a game; the rects given
    /// before a game starts reach its engine when it goes on screen, and those given while it
    /// runs at once; rects with no area are left out.
    #[test]
    fn menu_hidden_reads_back_the_rects_the_engine_holds() {
        let mut game = Game::new();
        game.set_menu_hidden(vec![10, 20, 30, 40, 5, 5, 0, 9, 1, 2, 3]);
        assert!(game.menu_hidden().is_empty(), "no game: none held");
        game.put_on_screen(synthetic_engine());
        assert_eq!(
            game.engine.as_ref().quiet().context_menu_hidden(),
            [(10, 20, 39, 59)],
            "given before the game: its engine holds the rect with area"
        );
        assert_eq!(game.menu_hidden(), [10, 20, 30, 40], "read from the engine");
        game.set_menu_hidden(vec![0, 0, 8, 8]);
        assert_eq!(
            game.menu_hidden(),
            [0, 0, 8, 8],
            "given while it runs: the engine holds it at once"
        );
    }

    /// Spec 21: `travel_to` starts the engine's travel exactly when
    /// [`u6_core::actors::travel::reachable`] lets it, and nothing before a start. On the user's
    /// zip (skipped when absent) at the first prompt, the 49 cells round the leader (the castle's
    /// fight is on there, so the refusals are what it shows; the walk itself is
    /// `u6-core`'s real-data receipt, `actors/travel_tests.rs`).
    #[test]
    fn travel_to_starts_exactly_what_reachable_allows() {
        let mut none = Game::new();
        assert!(!none.travel_to(1, 1) && !none.travel_active(), "no game");
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        let p = game.leader_position();
        let (mut why, mut any) = (std::collections::BTreeMap::new(), false);
        for dy in -3i16..=3 {
            for dx in -3i16..=3 {
                let c = (p[0].wrapping_add_signed(dx), p[1].wrapping_add_signed(dy));
                let reason = {
                    let e = game.engine.as_ref().quiet();
                    u6_core::actors::travel::reachable(&e.state(), c)
                };
                let started = game.travel_to(c.0, c.1);
                assert_eq!(started, reason.is_none(), "{dx} {dy}");
                any |= started;
                assert_eq!(
                    game.travel_active(),
                    any,
                    "a refusal leaves the travel as it was"
                );
                *why.entry(reason.map_or("started", |r| r.code()))
                    .or_insert(0) += 1;
            }
        }
        eprintln!("travel_to over 49 cells: {why:?}");
    }

    /// Spec 21 round 2 (review gpt-b MUST 1): past the fight, broken off as spec 21's receipt
    /// does it ([`castle_game`]), a reachable cell starts a travel; a refused request then (the
    /// leader's own cell) leaves that travel walking to its target, which it reaches. A held
    /// key's repeat queued the way the page queues it (into `pending`, before the step) ends a
    /// second travel in that step.
    #[test]
    fn travel_to_starts_a_reachable_cell_and_a_refusal_leaves_it_walking() {
        use u6_core::actors::travel::{End, reachable};
        let Some(mut game) = castle_game() else {
            return;
        };
        let p = game.leader_position();
        let to = [(2i16, 0i16), (-2, 0), (0, 2), (0, -2)]
            .into_iter()
            .map(|(dx, dy)| (p[0].wrapping_add_signed(dx), p[1].wrapping_add_signed(dy)))
            .find(|&c| reachable(&game.engine.as_ref().quiet().state(), c).is_none())
            .quiet();
        assert!(game.travel_to(to.0, to.1), "a reachable cell starts");
        assert!(game.travel_active());
        assert!(
            !game.travel_to(p[0], p[1]),
            "the leader's own cell is refused"
        );
        assert!(
            game.travel_active(),
            "the refusal leaves the travel walking"
        );
        let target = game
            .engine
            .as_ref()
            .quiet()
            .travel_progress()
            .quiet()
            .target();
        assert_eq!(target, to, "to its own target");
        for _ in 0..3000 {
            if !game.travel_active() {
                break;
            }
            game.step(20);
        }
        assert_eq!(
            game.engine.as_ref().quiet().travel_end(),
            Some(End::Arrived)
        );
        let at = game.leader_position();
        assert_eq!((at[0], at[1]), to, "on the target");
        assert!(steps_to_prompt(&mut game), "the prompt");
        assert!(game.travel_to(p[0], p[1]), "back");
        game.step(20);
        assert!(game.travel_active(), "walking back");
        game.pending.push(InputEvent::Repeat(Key::Left));
        game.step(20);
        assert_eq!(
            game.engine.as_ref().quiet().travel_end(),
            Some(End::Input),
            "a queued repeat ends it"
        );
        eprintln!(
            "travel_to past the fight: started, kept through a refusal, arrived; a repeat stopped the next"
        );
    }

    /// [`page_frame`]s 16 wall ms apart from `t` on (the page's clock given at each) until the
    /// travel ends (at most 2000 frames): the page clock at each step whose tick the command
    /// prompt took a press in.
    fn travel_takes(game: &mut Game, t: &mut f64) -> Vec<f64> {
        let mut at = Vec::new();
        for _ in 0..2000 {
            if !game.travel_active() {
                break;
            }
            *t += 16.0;
            game.set_clock(*t);
            let k = game.pace(16.0).min(40);
            for _ in 0..k {
                let takes = game.prompt_takes();
                game.step(step_ms());
                if game.prompt_takes() != takes {
                    at.push(*t);
                }
            }
            game.ran(k);
        }
        at
    }

    /// Spec 21 S3 (K2 r6 opus SHOULD 1, the merge round; round 5, gpt b SHOULD 1): at every speed,
    /// 1×, 4×, 8× and Max (where the page runs up to 40 steps a frame on one clock), a travel past
    /// the castle's fight steps no sooner than a `MIN_STEP_MS` of the page's clock after the press
    /// the prompt took last, as a held arrow does, the travel's own or the player's: its first step
    /// after entering clocked mode from headless stepping ([`castle_game`] steps with no clock, so
    /// its last take, the fight's `b`, stands at the page clock's 0; the clock then starts at 16
    /// ms), each step after the one before to its arrival, and the first step of a travel started
    /// right after an ordinary command the prompt took (Space, a pass). The least gap of each
    /// speed, printed. The entry into clocked mode never happens in production (round 6, opus NIT
    /// 4): the page gives `set_clock` before the first `step` of every frame (`site/app.js`), so no
    /// take is stamped without its clock; its 160 ms holds only because `Game`'s clock starts at
    /// 0.0, which is this test's `t = 0`.
    #[test]
    fn at_every_speed_a_travel_steps_at_most_once_a_min_step_of_the_pages_clock() {
        use u6_core::actors::travel::{End, reachable};
        let min = f64::from(u6_core::engine::MIN_STEP_MS);
        // Per speed: the first step after entering clocked mode, the least gap between steps, the
        // first step after the pass (page ms after the take before each).
        let mut seen = Vec::new();
        for speed in [1.0, 4.0, 8.0, 0.0] {
            let Some(mut game) = castle_game() else {
                return;
            };
            game.set_speed(speed);
            assert!(
                game.prompt_takes() > 0.0,
                "the fixture: headless, the prompt took the fight's key"
            );
            let p = game.leader_position();
            let to = [(3i16, 0i16), (-3, 0), (0, 3), (0, -3), (2, 2), (-2, -2)]
                .into_iter()
                .map(|(dx, dy)| (p[0].wrapping_add_signed(dx), p[1].wrapping_add_signed(dy)))
                .find(|&c| reachable(&game.engine.as_ref().quiet().state(), c).is_none())
                .quiet();
            assert!(game.travel_to(to.0, to.1), "a reachable cell starts");
            let mut t = 0.0;
            let at = travel_takes(&mut game, &mut t);
            assert_eq!(
                game.engine.as_ref().quiet().travel_end(),
                Some(End::Arrived),
                "speed {speed}: the travel arrived"
            );
            assert!(
                at.len() >= 3,
                "speed {speed}: three or more steps: {}",
                at.len()
            );
            // The ordinary command: Space at the prompt, the step that takes it at page clock
            // `pass`; a travel back started at once.
            for _ in 0..200 {
                if game.at_prompt() {
                    break;
                }
                t += 16.0;
                page_frame(&mut game, t, 16.0);
            }
            assert!(
                game.at_prompt(),
                "speed {speed}: the prompt after the arrival"
            );
            let takes = game.prompt_takes();
            assert!(game.key_down("Space", " ", false, t));
            let mut pass = None;
            for _ in 0..200 {
                t += 16.0;
                page_frame(&mut game, t, 16.0);
                if game.prompt_takes() != takes {
                    pass = Some(t);
                    break;
                }
            }
            let pass = pass.expect("the prompt took the pass");
            assert!(game.key_up("Space"));
            assert!(game.travel_to(p[0], p[1]), "speed {speed}: back");
            let back = travel_takes(&mut game, &mut t);
            assert!(!back.is_empty(), "speed {speed}: the travel back steps");
            let least = at
                .windows(2)
                .map(|w| w[1] - w[0])
                .fold(f64::INFINITY, f64::min);
            let (first, after) = (at[0], back[0] - pass);
            println!(
                "speed {speed}: entering clocked mode, the first step at page ms {first} (the last \
                 headless take at 0); {} steps, the least gap {least} page ms; after a pass, the \
                 first step {after} page ms on",
                at.len()
            );
            seen.push((speed, first, least, after));
        }
        let soon: Vec<_> = seen
            .iter()
            .filter(|&&(_, first, least, after)| first < min || least < min || after < min)
            .collect();
        assert!(
            soon.is_empty(),
            "a step sooner than 150 page ms after the take before it (speed, after entering \
             clocked mode, the least gap, after the pass): {soon:?}"
        );
    }

    /// The castle start of spec 19's held-key tests in a `Game` (as `u6-core`'s travel receipt):
    /// `game` at once with seed 0 round the shared state, the actors in the assault worktype 8
    /// passed over (status bit 0x10), the fight broken off with `b`, at the prompt. Skipped when
    /// the user's zip is absent, unless `U6_REQUIRE_ARCHIVE=1`, which fails the test instead.
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn castle_game() -> Option<Game> {
        if !u6_testkit::archive().exists() {
            assert_ne!(
                std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
                Ok("1"),
                "U6_REQUIRE_ARCHIVE=1 and the archive (U6_ARCHIVE) is missing"
            );
            eprintln!("skipped: the archive (U6_ARCHIVE) not present");
            return None;
        }
        let r = intake(vec![InputFile {
            name: "ultima-vi.zip".into(),
            data: std::fs::read(u6_testkit::archive()).quiet(),
        }]);
        let files = Files::new(r.files);
        let mut state = u6_core::State::new_game(&files).quiet();
        state.rng.seed(0);
        let keep = Rc::new(std::cell::RefCell::new(None));
        let k = Rc::clone(&keep);
        let mut game = Game::new();
        game.engine = Some(Engine::with_game(state, files, move |st, io| {
            *k.borrow_mut() = Some(Rc::clone(&st));
            u6_core::main_loop::game_first(st, io)
        }));
        assert!(steps_to_prompt(&mut game), "the prompt");
        let st = keep.borrow_mut().take().quiet();
        {
            let mut s = st.borrow_mut();
            for a in 0..0x100 {
                if s.actors.worktype[a] == 8 {
                    s.actors.attrs.status[a] |= 0x10;
                }
            }
        }
        game.pending.push(InputEvent::Key(Key::Char('b')));
        for _ in 0..25 {
            game.step(20);
        }
        assert!(steps_to_prompt(&mut game), "the prompt past the fight");
        Some(game)
    }

    /// The user's zip booted `game` first, stepped to its prompt; none (the test skips, saying
    /// so) without it, unless `U6_REQUIRE_ARCHIVE=1` (the release receipt), which fails the test
    /// instead.
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn at_the_prompt() -> Option<Game> {
        if !u6_testkit::archive().exists() {
            assert_ne!(
                std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
                Ok("1"),
                "U6_REQUIRE_ARCHIVE=1 and the archive (U6_ARCHIVE) is missing"
            );
            eprintln!("skipped: the archive (U6_ARCHIVE) not present");
            return None;
        }
        let mut game = Game::new();
        game.collect(
            "ultima-vi.zip",
            std::fs::read(u6_testkit::archive()).quiet(),
        )
        .quiet();
        game.boot(1, true).quiet();
        assert!(steps_to_prompt(&mut game), "the prompt");
        Some(game)
    }

    /// Steps `game` (20 ms each, at most 3000) until the prompt is up; false if it never is. The
    /// prompt is [`Game::at_prompt`]'s (A4-0x7a2c, nothing queued), not the context menu's gate,
    /// which also needs keys taken as commands and no cursor up ([`Engine::context_menu_gate`]).
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
        assert!(game.key_down("KeyD", "d", false, 0.0), "Drop's key");
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
            assert!(
                game.key_down("Escape", "Escape", false, 0.0),
                "Escape taken"
            );
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

    /// Spec 17 Acceptance 5 (round 2, item 10), on the user's zip: the leader starts without a
    /// readied spellbook, so on a cell two to five steps off that shows it ([`cell_point`],
    /// either build's layout) the drawn menu has Cast disabled as book; a left press on it, its
    /// letter and Enter with it highlighted run nothing (the menu stays open, no button and no
    /// line reach the game, the leader stays); Escape closes it. (The run of a Cast forced through
    /// the engine anyway, its target press dropped: u6-core's `objects/commands/real_data.rs`,
    /// `cast_without_a_book_is_disabled_and_forced_its_target_press_is_dropped`.)
    #[test]
    fn a_disabled_entry_runs_nothing() {
        let Some(mut game) = at_the_prompt() else {
            return;
        };
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
        let lines = |g: &Game| g.engine.as_ref().quiet().state().ui.messages.len();
        let before = lines(&game);
        click(&mut game, (x, y), 2);
        let v = drawn(&game).expect("open");
        let cast = row_of(&v, 0x82).expect("Cast shown");
        assert!(!v.rows[cast].enabled(), "disabled");
        click(&mut game, row_point(&v, cast), 1);
        assert!(drawn(&game).is_some(), "a press on it: still open");
        assert!(game.key_down("KeyC", "c", false, 0.0));
        game.step(20);
        game.key_up("KeyC");
        game.step(20);
        assert!(drawn(&game).is_some(), "its letter: still open");
        for _ in 0..v.rows.len() {
            if drawn(&game).is_none_or(|v| v.hover == cast) {
                break;
            }
            assert!(game.key_down("ArrowDown", "ArrowDown", false, 0.0));
            game.step(20);
            game.key_up("ArrowDown");
            game.step(20);
        }
        assert_eq!(
            drawn(&game).map(|v| v.hover),
            Some(cast),
            "Cast highlighted"
        );
        assert!(game.key_down("Enter", "Enter", false, 0.0));
        game.step(20);
        game.key_up("Enter");
        game.step(20);
        assert!(drawn(&game).is_some(), "Enter on it: still open");
        assert_eq!(game.game_buttons(), 0, "no button reached the game");
        assert!(game.key_down("Escape", "Escape", false, 0.0));
        game.step(20);
        game.key_up("Escape");
        for _ in 0..30 {
            game.step(20);
        }
        println!("the cell {dx}, {dy} from the hero");
        assert!(
            drawn(&game).is_none() && game.at_prompt(),
            "closed, the prompt up"
        );
        assert_eq!(lines(&game), before, "no line printed");
        assert_eq!(game.leader_position(), at, "the leader did not move");
    }

    /// Spec 17 (round 2, gpt f2 SHOULD 1), on the user's zip: a game replaced while its menu is
    /// open (a restart: a new engine on screen) leaves no menu and no press of the old one: the
    /// held right button's release reaches the new game as a move, which holds no button, and a
    /// right press at its prompt opens its own menu.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn a_game_replaced_while_its_menu_is_open_keeps_nothing_of_it() {
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        let (hx, hy) = hero_point();
        game.pointer(hx, hy, 2);
        game.step(20);
        assert!(drawn(&game).is_some(), "open");
        game.collect(
            "ultima-vi.zip",
            std::fs::read(u6_testkit::archive()).quiet(),
        )
        .quiet();
        game.boot(1, true).quiet();
        assert_eq!(game.menu_view(), "", "the new game: no menu");
        game.pointer(hx, hy, 0);
        game.step(20);
        assert_eq!(game.game_buttons(), 0, "the old press's release: no button");
        assert!(steps_to_prompt(&mut game), "the new game's prompt");
        click(&mut game, (hx, hy), 2);
        assert!(drawn(&game).is_some(), "its own menu");
    }

    /// Spec 17 Presentation in the platform: a cell drawn half-toned (a disabled entry of the
    /// context menu) is listed as [`HALFTONE`] | its code; one drawn in full as its code.
    #[test]
    fn the_hd_glyphs_flag_a_half_toned_cell() {
        let mut game = Game::new();
        let engine = Engine::with_game(
            u6_core::State::empty(),
            Files::default(),
            |_, io| async move {
                use u6_core::ui::text::{self, TextWindow};
                let font = u6_formats::font::BitFont {
                    glyphs: (0..=255u8).map(|c| [c; 8]).collect(),
                };
                let w = TextWindow::new(0, 0, 39, 24, 3, 4, 0);
                io.draw(|fb| {
                    for (i, c) in fb.palette.iter_mut().enumerate() {
                        let v = i as u8;
                        *c = u6_core::Rgb::new(v.wrapping_mul(40), 255 - v, v.wrapping_mul(7));
                    }
                    for y in 0..u6_render::HEIGHT {
                        for x in 0..u6_render::WIDTH {
                            fb.set(x, y, 4);
                        }
                    }
                    text::draw_char(fb, Some(&font), None, &w, 0x41, 2, 2);
                    text::draw_halftone(fb, (Some(&font), None), &w, 0x55, (3, 2));
                });
                io.present();
                loop {
                    io.vblank().await;
                }
            },
        );
        let mut engine = engine;
        engine.enable_src_plane();
        game.put_on_screen(engine);
        game.step(20);
        game.present();
        game.set_hd(true, 2);

        let codes: Vec<u32> = game.hd_glyphs().chunks(5).map(|g| g[2]).collect();
        assert_eq!(
            codes,
            [0x41, HALFTONE | 0x55],
            "the letter, the half-toned letter"
        );
    }

    /// Spec 17 round 2, item 0: a key let go just before a right press on the hero ([`hero_point`])
    /// (its release queued, as `key_up` queues it, in the same step) does not keep the menu shut,
    /// and the entry chosen then runs: the release is applied where the engine queues the input
    /// and is never one of its events. Look is chosen by its letter.
    #[test]
    fn a_key_let_go_just_before_a_right_press_keeps_the_menu_and_its_run() {
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        assert!(game.key_down("KeyX", "x", false, 0.0));
        game.step(20);
        assert!(steps_to_prompt(&mut game), "the prompt after the key");
        let (hx, hy) = hero_point();
        assert!(game.key_up("KeyX"), "the key was held");
        game.pointer(hx, hy, 2);
        game.step(20);
        game.pointer(hx, hy, 0);
        game.step(20);
        assert!(
            drawn(&game).is_some(),
            "the menu with the release queued before the press"
        );
        assert!(game.key_down("KeyL", "l", false, 0.0), "Look's letter");
        game.step(20);
        game.key_up("KeyL");
        assert!(drawn(&game).is_none(), "chosen: closed");
        let mut read = 0;
        for _ in 0..200 {
            game.step(20);
            if !game.at_prompt() {
                read = 1;
                break;
            }
        }
        assert_eq!(read, 1, "Look read its target");
    }

    /// Spec 19 O1c on the user's zip: the castle start state of u6-core's held-key tests
    /// (`ui/tests/held_key.rs`: `game` at once with seed 0, the actors in the assault worktype 8
    /// passed over with status bit 0x10, the fight broken off with `b`), at the prompt. Without
    /// the zip the test that asked is skipped (None), and says so; with `U6_REQUIRE_ARCHIVE=1`
    /// it fails instead (round 5, review b-1). Each use prints whether it ran.
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn in_the_castle() -> Option<Game> {
        castle_with_state().map(|(mut game, _)| {
            // Existing interpolation pixel oracles exercise the original art policy.
            // Footstep receipts use castle_with_state and the shipping B default explicitly.
            game.set_footstep_pattern(0);
            game
        })
    }

    /// [`in_the_castle`], with the game's state kept for the test to write.
    #[allow(clippy::disallowed_methods)] // tests may read files and the environment
    fn castle_with_state() -> Option<(Game, Rc<std::cell::RefCell<u6_core::State>>)> {
        use std::cell::RefCell;
        if !u6_testkit::archive().exists() {
            let required = std::env::var("U6_REQUIRE_ARCHIVE").is_ok_and(|v| v == "1");
            assert!(
                !required,
                "U6_REQUIRE_ARCHIVE=1 and the archive (U6_ARCHIVE) not present"
            );
            eprintln!("castle fixture: skipped (the archive (U6_ARCHIVE) not present)");
            return None;
        }
        eprintln!("castle fixture: ran");
        let r = intake(vec![InputFile {
            name: "ultima-vi.zip".into(),
            data: std::fs::read(u6_testkit::archive()).quiet(),
        }]);
        let files = Files::new(r.files);
        let mut state = u6_core::State::new_game(&files).quiet();
        state.rng.seed(0);
        let keep = Rc::new(RefCell::new(None));
        let k = Rc::clone(&keep);
        let mut game = Game::new();
        game.engine = Some(Engine::with_game(state, files, move |st, io| {
            *k.borrow_mut() = Some(Rc::clone(&st));
            u6_core::main_loop::game_first(st, io)
        }));
        assert!(steps_to_prompt(&mut game), "the prompt");
        let st = keep.borrow_mut().take().quiet();
        {
            let mut s = st.borrow_mut();
            for a in 0..0x100 {
                if s.actors.worktype[a] == 8 {
                    s.actors.attrs.status[a] |= 0x10;
                }
            }
        }
        assert!(game.key_down("KeyB", "b", false, 0.0));
        assert!(game.key_up("KeyB"));
        for _ in 0..24 {
            game.step(20);
        }
        assert!(steps_to_prompt(&mut game), "the prompt after the break-off");
        Some((game, st))
    }

    /// The command loop reads a command key (A4-0x7a2c set).
    fn reading(game: &Game) -> bool {
        game.engine
            .as_ref()
            .is_some_and(|e| e.state().main.reading == 1)
    }

    /// Presses the key `code` (also its DOM `key`) now on the page's clock, as its event would.
    fn down(game: &mut Game, code: &str) -> bool {
        let at = game.clock;
        game.key_down(code, code, false, at)
    }

    /// As the browser's auto-repeat of `code` now.
    fn repeat(game: &mut Game, code: &str) -> bool {
        let at = game.clock;
        game.key_repeat(code, code, false, at)
    }

    /// One step at 1× as the page runs it: a 20 ms frame on the page's clock
    /// ([`Game::set_clock`], [`Game::pace`]), the step, [`Game::ran`].
    fn step_1x(game: &mut Game) {
        game.set_clock(game.clock + 20.0);
        game.pace(20.0);
        game.step(20);
        game.ran(1);
    }

    /// One display frame of `dt` wall ms as the page runs it (spec 06): the page's clock moved
    /// on, the steps [`Game::pace`] gives at the speed, each followed by `seen`, then
    /// [`Game::ran`].
    fn frame(game: &mut Game, dt: f64, seen: &mut impl FnMut(&Game)) {
        game.set_clock(game.clock + dt);
        let k = game.pace(dt);
        for _ in 0..k {
            game.step(20);
            seen(game);
        }
        game.ran(k);
    }

    /// 60 Hz display frames, in wall ms.
    const FRAME: f64 = 1000.0 / 60.0;

    /// The held walk's minimum interval, in wall ms.
    const MIN: f64 = u6_core::engine::MIN_STEP_MS as f64;

    /// What a hold of the down arrow did ([`hold_south`]): per step of the leader, the step()
    /// calls from the command loop reading again to the step (None where it never read between
    /// two steps, a key already waiting), and the engine ms since the step before; the steps
    /// after the release; the events still queued at the end.
    #[derive(Debug)]
    struct Hold {
        from_prompt: Vec<Option<usize>>,
        gaps_ms: Vec<u64>,
        after: usize,
        queued: usize,
    }

    /// Holds the down arrow from the prompt at 1× (south, the way open for about ten cells in
    /// both builds; its release by `key_up`; with `repeats`, the browser's repeat too: the first
    /// 225 ms after the press, then one every 30 ms) until the leader has stepped `steps` times
    /// or 30 s passed, lets it go, and steps 1.5 s more.
    fn hold_south(game: &mut Game, steps: usize, repeats: bool) -> Hold {
        let now = |g: &Game| g.engine.as_ref().map_or(0, |e| e.io().now_ms());
        let t0 = now(game);
        assert!(down(game, "ArrowDown"));
        let mut next_repeat = 225;
        let mut at = game.leader_position();
        let (mut read_at, mut last_ms) = (Some(0), t0);
        let mut h = Hold {
            from_prompt: Vec::new(),
            gaps_ms: Vec::new(),
            after: 0,
            queued: 0,
        };
        for i in 1..=1500 {
            if h.from_prompt.len() == steps {
                break;
            }
            while repeats && now(game) - t0 + 20 > next_repeat {
                assert!(repeat(game, "ArrowDown"));
                next_repeat += 30;
            }
            step_1x(game);
            if game.leader_position() != at {
                at = game.leader_position();
                h.from_prompt.push(read_at.take().map(|r| i - r));
                h.gaps_ms.push(now(game) - last_ms);
                last_ms = now(game);
            } else if read_at.is_none() && reading(game) {
                read_at = Some(i);
            }
        }
        assert!(game.key_up("ArrowDown"));
        for _ in 0..75 {
            step_1x(game);
            if game.leader_position() != at {
                at = game.leader_position();
                h.after += 1;
            }
        }
        h.queued = game.keys_pending() as usize;
        h
    }

    /// Spec 19 O1c (the owner, 2026-10-27: a held arrow steps once, waits out the OS repeat
    /// delay, then walks on; "누르면 즉시 계속 이동하도록해줘"): the down arrow held with no
    /// repeat from the browser walks three steps in three turns, the second and the third each
    /// in the step() call after the command loop reads again (at 1× a turn outlasts the minimum
    /// interval); its release ends the walk.
    #[test]
    fn a_held_arrow_walks_on_at_each_prompt_without_the_browsers_repeat() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        let h = hold_south(&mut game, 3, false);
        println!("held down, no repeats: {h:?}");
        assert_eq!(h.from_prompt.len(), 3, "three steps while held");
        assert_eq!(
            h.from_prompt[1..],
            [Some(1), Some(1)],
            "each taken in the step after the prompt came back"
        );
        assert_eq!((h.after, h.queued), (0, 0), "none after the release");
    }

    /// Spec 19 O1c with the browser's repeat arriving too: still one step a turn (no two
    /// within the shortest castle turn, 620 ms), and nothing left queued or walked after the
    /// release.
    #[test]
    fn a_held_arrow_with_the_browsers_repeat_still_steps_once_a_turn() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        let h = hold_south(&mut game, 3, true);
        println!("held down, with repeats: {h:?}");
        assert_eq!(h.from_prompt.len(), 3, "three steps while held");
        assert!(
            h.gaps_ms[1..].iter().all(|&ms| ms >= 600),
            "one step a turn: {:?}",
            h.gaps_ms
        );
        assert_eq!((h.after, h.queued), (0, 0), "none after the release");
    }

    /// Spec 19 O1c: a tap of the arrow (its press and release in one step) is one step.
    #[test]
    fn a_tapped_arrow_steps_once() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        let mut at = game.leader_position();
        assert!(down(&mut game, "ArrowDown"));
        assert!(game.key_up("ArrowDown"));
        let mut steps = 0;
        for _ in 0..150 {
            step_1x(&mut game);
            if game.leader_position() != at {
                at = game.leader_position();
                steps += 1;
            }
        }
        assert_eq!(steps, 1, "one step");
        assert!(game.at_prompt(), "the prompt again");
    }

    /// The down arrow pressed at `speed` (0: Max) and held `hold` wall ms of 60 Hz frames, then
    /// 2 s of frames more: per step of the leader, the page's clock from the press (wall ms) and
    /// whether it came while the key was down; the step() calls from the press to the first.
    fn press_for(game: &mut Game, speed: f64, hold: f64) -> (Vec<(f64, bool)>, usize) {
        game.set_speed(speed);
        let t0 = game.clock;
        let mut at = game.leader_position();
        let mut cells = Vec::new();
        let (mut calls, mut first) = (0usize, None);
        assert!(down(game, "ArrowDown"));
        let mut down = true;
        let mut count = |g: &Game| {
            calls += 1;
            if g.leader_position() != at {
                at = g.leader_position();
                cells.push((g.clock - t0, down));
                first.get_or_insert(calls);
            }
        };
        while game.clock - t0 < hold {
            frame(game, FRAME, &mut count);
        }
        assert!(game.key_up("ArrowDown"));
        down = false;
        let mut count = |g: &Game| {
            if g.leader_position() != at {
                at = g.leader_position();
                cells.push((g.clock - t0, down));
            }
        };
        for _ in 0..120 {
            frame(game, FRAME, &mut count);
        }
        (cells, first.unwrap_or(0))
    }

    /// Spec 19 O1c (round 3, D1'), a tap and the cadence: a 120 wall ms tap, held over several
    /// frames, is one cell at 1×, 8× and Max (at 8× and Max a prompt comes back within the
    /// tap, and the minimum interval keeps it from walking on); a 600 ms hold at 8× walks
    /// floor(600 / 150) + 1 cells, within one, at the minimum interval (the castle's turns are
    /// 80 to 108 wall ms there).
    #[test]
    fn a_tap_is_one_cell_at_any_speed_and_a_hold_walks_at_the_minimum_interval() {
        for speed in [1.0, 8.0, 0.0] {
            let Some(mut game) = in_the_castle() else {
                return;
            };
            let (cells, _) = press_for(&mut game, speed, 120.0);
            println!("tap 120 wall ms at speed {speed}: {} cell(s)", cells.len());
            assert_eq!(cells.len(), 1, "one cell at speed {speed} (0: Max)");
        }
        let Some(mut game) = in_the_castle() else {
            return;
        };
        let (cells, _) = press_for(&mut game, 8.0, 600.0);
        let held = cells.iter().filter(|(_, down)| *down).count();
        println!("hold 600 wall ms at 8x: {held} cell(s) while down, {cells:?}");
        let want = (600.0 / MIN) as usize + 1;
        assert!(
            held.abs_diff(want) <= 1,
            "{held} cells, want {want} within one"
        );
        assert!(
            cells.iter().all(|(_, down)| *down),
            "none after the release"
        );
    }

    /// Spec 19 O1c (round 3, D1'), the owner's symptom above 4.3×: at 6× and 8× every castle
    /// turn is shorter than the minimum interval (640 / 860 engine ms are 107 / 143 wall ms at
    /// 6×, 80 / 108 at 8×), so a held walk steps at an even max(turn / speed, 150) = 150 wall
    /// ms rounded to a frame: every gap between two steps, the first one included (the interval
    /// runs from the step the prompt last took), is 150 within one frame (the round-2 gate on
    /// the first press alone gave 150 ms, then turn / speed). At Max too, both ways (round 4
    /// item 6): one paced press per 150 ms, no more and no fewer.
    #[test]
    fn above_4x_a_held_walk_keeps_an_even_cadence_and_max_is_rate_capped() {
        for speed in [6.0, 8.0, 0.0] {
            let Some(mut game) = in_the_castle() else {
                return;
            };
            let (cells, _) = press_for(&mut game, speed, 1000.0);
            let held: Vec<f64> = cells.iter().filter(|c| c.1).map(|c| c.0).collect();
            let gaps: Vec<f64> = held.windows(2).map(|w| w[1] - w[0]).collect();
            let shown: Vec<f64> = gaps.iter().map(|g| g.round()).collect();
            println!(
                "held 1 s at speed {speed}: {} steps, gaps {shown:?} wall ms",
                held.len()
            );
            // The frames' clock is a sum of 1000 / 60 steps: a little slack for its rounding.
            let frame = FRAME + 0.01;
            assert!(gaps.len() >= 4, "steps while held at speed {speed}");
            assert!(
                gaps.iter().all(|&g| (g - MIN).abs() <= frame),
                "an even 150 ms at speed {speed} (0: Max): {shown:?}"
            );
        }
    }

    /// Whether a slide's view draws anything off rest: the camera or a mover off its place.
    fn off_rest(view: &View) -> bool {
        view.camera != (0, 0) || view.movers.iter().any(|m| m.offset != (0, 0))
    }

    /// How far a placement lies behind rest on a step's side, in pixels: `k` when `offset` is
    /// `-k` cells-worth along the step `dir` (one cell each way at most, Chebyshev) with
    /// 0 < k ≤ 16, the place between the step's two cells; `None` anywhere else (on the far side
    /// of rest, past the old cell, off the step's line), and at rest.
    fn behind(offset: (i32, i32), (dx, dy): (i32, i32)) -> Option<i32> {
        let k = if dx != 0 {
            -offset.0 * dx
        } else {
            -offset.1 * dy
        };
        ((1..=16).contains(&k) && offset == (-k * dx, -k * dy)).then_some(k)
    }

    /// One step's frames ([`Progress::frame`]), by where the map is drawn (natively the camera,
    /// in the page the static layer): the frames to the next step, those with the map off rest,
    /// at rest while the presenter draws (an early picture held, or other objects sliding),
    /// moving (closer to rest along the step than on the step's frame before that reached the
    /// screen, which was between its two cells too), backward (a frame that reached the screen
    /// farther from rest than the last that did, once the step's slide began: a frame at rest
    /// before its first frame off rest is before its onset, not a place it went back from; round
    /// 13: judged against that last place, the engine's frame at rest included, so a slide that
    /// starts again after rest counts) and stray (off rest but not between the step's two
    /// cells). And the jump (round 12, opus S1): the largest drop of the map's offset behind rest
    /// between two frames that reached the screen, from 16 (the old cell, where the step starts)
    /// to its first; a frame of the engine's after the slide began counts as at rest (0), one
    /// before it is not counted, and (round 13) one of the engine's that shows the map as the
    /// engine published it before the step, a foreign picture after the step's early picture with
    /// no scene since ([`Game::old_cell`]), counts as the old cell (16). So a slide cut short (16,
    /// 15, 14, then the engine's frame) jumps 14, and one drawn in larger steps by their size.
    /// At the step's close ([`Progress::close`]: the next step, or the end of the sampling) the
    /// map must stand at rest once the slide began: a slide that stops short of rest (16, 15, 14
    /// and 14 on to the next step) is unfinished.
    #[derive(Debug)]
    struct Progress {
        dir: (i32, i32),
        frames: usize,
        off_rest: usize,
        held: usize,
        moving: usize,
        backward: usize,
        stray: usize,
        /// Frames of the engine's that showed the old cell ([`Game::old_cell`]).
        old_cell: usize,
        /// How far behind rest the step's frame before that reached the screen was ([`behind`];
        /// `Some(0)` at rest).
        before: Option<i32>,
        /// A frame off rest reached the screen: the step's slide began.
        began: bool,
        /// How far behind rest the map was drawn on the last frame that reached the screen (16
        /// until the slide began), and the largest drop of it so far.
        last: i32,
        jump: i32,
        /// Closed with the slide begun and the map not at rest.
        unfinished: bool,
    }

    impl Progress {
        fn new(dir: (i32, i32)) -> Self {
            Self {
                dir,
                frames: 0,
                off_rest: 0,
                held: 0,
                moving: 0,
                backward: 0,
                stray: 0,
                old_cell: 0,
                before: None,
                began: false,
                last: 16,
                jump: 0,
                unfinished: false,
            }
        }

        /// A display frame of the step showing `placed`: the placement's offset from rest while
        /// the presenter's picture is shown (`None`: the engine's frame); `old`: the engine's frame
        /// shows the old cell ([`Game::old_cell`]); `moved`: whether the frame's picture reached
        /// the screen (only such a frame counts as moving or backward, and only it is the frame
        /// before of the next).
        fn frame(&mut self, placed: Option<(i32, i32)>, old: bool, moved: bool) {
            self.frames += 1;
            let k = match placed {
                None if old => {
                    self.old_cell += 1;
                    Some(16)
                }
                None => None,
                Some((0, 0)) => {
                    self.held += 1;
                    Some(0)
                }
                Some(offset) => {
                    self.off_rest += 1;
                    let k = behind(offset, self.dir);
                    if k.is_none() {
                        self.stray += 1;
                        if moved {
                            self.before = None;
                        }
                        return;
                    }
                    k
                }
            };
            if !moved {
                return;
            }
            if let (Some(k), Some(b)) = (k, self.before)
                && k > 0
                && b > k
                && b <= 16
            {
                self.moving += 1;
            }
            if k.is_some_and(|k| self.began && k > self.last) {
                self.backward += 1;
            }
            self.before = k;
            self.began |= k.is_some_and(|k| k > 0);
            // Once the slide began, the engine's frame is the map at rest.
            if let (true, Some(at)) = (self.began, k.or(Some(0))) {
                self.jump = self.jump.max(self.last - at);
                self.last = at;
            }
        }

        /// The step ends (the next step, or the end of the sampling): once its slide began the
        /// map stands at rest, else the step is unfinished.
        fn close(&mut self) {
            self.unfinished = self.began && self.last != 0;
        }
    }

    /// The largest jump (drop of the map's offset between two display frames, [`Progress`]) a
    /// slide at `rate` engine ms per wall ms on 60 Hz frames may make: ceil(16 · Δ / D), Δ the
    /// engine ms a display frame runs and D the slide's 360 ms: 1 at 1×, 3 at 4×, 6 at 8× (round
    /// 13, opus N4: round 12's + 1 was slack). The presentation clock moves Δ a frame (the
    /// pacer's carried fraction takes up the ticks' 20 ms), and the slide's first frame lies
    /// within Δ of its start (it starts at the engine time of a tick of that frame or the one
    /// before, which the frame before showed), so the map moves at most 16 · Δ / D a frame; the
    /// offsets are rounded, and round(a) − round(a − x) is an integer under x + 1, so at most
    /// ceil(x).
    fn jump_bound(rate: f64) -> i32 {
        (16.0 * rate * FRAME / u6_render::motion::SLIDE_MS).ceil() as i32
    }

    /// The receipts' predicate on hand-made frames (rounds 12–13): a slide at 1× ([`jump_bound`]
    /// at rate 1) moving 16 to 0 a pixel or so a frame passes; one drawn in 4 px steps jumps 4; one cut
    /// short (16, 15, 14, then the engine's frame) jumps 14; one frozen has no moving frame; one
    /// stopped short of rest to the step's close (16, 15, 14, 14, 14) is unfinished; one run
    /// backward or restarted after reaching rest counts backward, after an engine frame at rest
    /// too (15, 0, the engine's, 10); a frame at rest before the slide's first frame off rest is
    /// its onset, not backward; a frame that did not reach the screen is neither moving, nor
    /// backward (15 shown, 16 not, 14 shown), nor the frame before of the next; an engine frame
    /// that shows the old cell counts as 16: backward, and its drop to rest a jump. The same
    /// cases run through the page's twin (test-web.mjs `progress`) at the start of its test.
    #[test]
    fn the_step_receipt_counts_the_jump_onset_and_frames_not_shown() {
        // A frame: how far behind rest (None: the engine's frame), the old cell, shown.
        let run_old = |frames: &[(Option<i32>, bool, bool)]| {
            let mut p = Progress::new((1, 0));
            for &(k, old, moved) in frames {
                p.frame(k.map(|k| (-k, 0)), old, moved);
            }
            p.close();
            p
        };
        let run = |frames: &[(Option<i32>, bool)]| {
            run_old(
                &frames
                    .iter()
                    .map(|&(k, m)| (k, false, m))
                    .collect::<Vec<_>>(),
            )
        };
        let shown = |ks: &[Option<i32>]| ks.iter().map(|&k| (k, true)).collect::<Vec<_>>();
        let bound = jump_bound(1.0);
        assert_eq!(
            (bound, jump_bound(4.0), jump_bound(8.0)),
            (1, 3, 6),
            "the bounds"
        );
        let smooth: Vec<Option<i32>> = (0..=22)
            .map(|f| {
                let k = (16.0 - 16.0 * f64::from(f) * FRAME / 360.0).round() as i32;
                Some(k.max(0))
            })
            .chain([None])
            .collect();
        let p = run(&shown(&smooth));
        assert!(
            p.jump <= bound
                && p.moving >= 2
                && p.backward == 0
                && p.stray == 0
                && !p.unfinished
                && p.old_cell == 0,
            "smooth: {p:?}"
        );
        let p = run(&shown(&[
            Some(16),
            Some(12),
            Some(8),
            Some(4),
            Some(0),
            None,
        ]));
        assert_eq!(p.jump, 4, "4 px steps jump 4");
        let p = run(&shown(&[Some(15), Some(14), None, None]));
        assert_eq!(p.jump, 14, "cut short: 14");
        let p = run(&shown(&[Some(12), Some(12), Some(12)]));
        assert_eq!(p.moving, 0, "frozen: none moving");
        let p = run(&shown(&[Some(16), Some(15), Some(14), Some(14), Some(14)]));
        assert!(
            p.unfinished && p.moving == 2 && p.backward == 0 && p.jump == 1,
            "stopped short of rest: {p:?}"
        );
        let p = run(&shown(&[Some(10), Some(12)]));
        assert_eq!(p.backward, 1, "backward");
        let p = run(&shown(&[Some(15), Some(0), Some(10)]));
        assert_eq!(p.backward, 1, "back off rest after reaching it");
        let p = run(&shown(&[Some(15), Some(0), None, Some(10)]));
        assert_eq!(
            p.backward, 1,
            "back off rest after the engine's frame: {p:?}"
        );
        let p = run(&[(Some(15), true), (Some(16), false), (Some(14), true)]);
        assert!(
            p.backward == 0 && p.moving == 1,
            "a frame not shown is not backward: {p:?}"
        );
        let p = run_old(&[
            (Some(15), false, true),
            (Some(14), false, true),
            (None, true, true),
            (None, true, true),
            (None, false, true),
        ]);
        assert!(
            p.old_cell == 2 && p.backward == 1 && p.jump == 16 && !p.unfinished,
            "the old cell through a foreign picture: {p:?}"
        );
        let p = run(&shown(&[None, Some(0), Some(15), Some(14)]));
        assert!(
            p.backward == 0 && p.moving == 1 && p.jump == 1,
            "onset after a frame at rest: {p:?}"
        );
        let p = run(&[(Some(15), true), (Some(13), false), (Some(14), true)]);
        assert!(
            p.moving == 1 && p.backward == 0 && p.jump == 1,
            "against the frame shown, not the one sampled: {p:?}"
        );
    }

    /// The moving frames a paced step must show at `rate` engine ms per wall ms on 60 Hz frames:
    /// two, or fewer where the slide is too short to be sure of more. A slide starts at the step's
    /// tick and the step's first frame draws it within one display frame of engine time (Δ, the
    /// engine ms a display frame runs) after it, so at least ceil(D / Δ − 1) of the step's frames
    /// draw it off rest (D the slide's 360 ms), and the first of them has no frame of the step
    /// before it. At 8× (Δ 133 ms: the slide is 45 wall ms, 2.7 display frames) that is two
    /// frames off rest, so one moving frame; `slides_shown` shows no slide under two display
    /// frames.
    fn moving_needed(rate: f64) -> usize {
        let frames = (u6_render::motion::SLIDE_MS / (rate * FRAME) - 1.0).ceil() as usize;
        frames.saturating_sub(1).min(2)
    }

    /// W3 uses the actually presented world positions. Targets are passed, retargeted or
    /// explicitly cancelled on direction changes; no rest is invented at a publication.
    #[test]
    fn a_held_walk_slides_on_every_paced_step_and_its_moving_share_by_speed() {
        held_walk_receipt(None, &[(true, true), (true, false), (false, false)]);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // Read-only owner locale pack; never emitted.
    fn an_owned_korean_quiet_held_walk_has_the_same_continuity_contract() {
        let path = u6_testkit::data_path_with("U6_LOCALE", "i18n/ko-1027a/ko.u6lp");
        if !path.is_file() {
            assert!(
                std::env::var_os("U6_LOCALE").is_none(),
                "explicit U6_LOCALE input is missing"
            );
            eprintln!("skipped: optional local Korean locale pack; set U6_LOCALE to require it");
            return;
        }
        let pack = load_locale(&std::fs::read(path).quiet()).quiet();
        held_walk_receipt(Some(&pack), &[(true, false)]);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ContinuousInput {
        Keyboard,
        Pointer,
        Travel,
        Menu,
        Stop,
        Detour,
    }

    #[test]
    fn footsteps_hd_raster_matches_classic_plan_and_refreshes_idle_shots() {
        let Some((mut game, state)) = castle_with_state() else {
            return;
        };
        for n in 0..256 {
            if state.borrow().actors.attrs.status[n] & 0x80 == 0 {
                state.borrow_mut().actors.attrs.status[n] |= 0x10;
            }
        }
        game.set_motion(true);
        game.set_hd(true, 2);
        let records: Vec<u16> = {
            let s = state.borrow();
            let kind = s.objects.type_frame[1] & 0x3ff;
            let base = s.tables.basetile.base[usize::from(kind)];
            (base..base + 16)
                .map(|t| s.gfx.record_of[usize::from(t)])
                .collect()
        };
        let images: Vec<_> = records
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|r| pack_tile(r, key_of(&game, r), 7))
            .collect();
        let (pack, _) = loaded(&images);
        game.set_hd_pack(&pack);
        let mut page = Page::new();
        let mut checked_lift = false;
        let mut replaced = 0;
        let mut previous_source = None;
        let mut previous_layers = 0.0;
        for frame in 0..160 {
            if frame == 30 {
                assert!(down(&mut game, "ArrowDown"));
            }
            if frame == 90 {
                game.key_up("ArrowDown");
            }
            page_frame_at(&mut game, &mut page, &mut |_| {});
            let source = game.motion.presenter.history().first().map(Rc::as_ptr);
            if frame < 30
                && source != previous_source
                && previous_source.is_some()
                && !game.motion.poses.is_empty()
            {
                assert!(
                    game.slide_layers_version() > previous_layers,
                    "idle environment changes cannot remain hidden under cached layers"
                );
                replaced += 1;
            }
            previous_source = source;
            previous_layers = game.slide_layers_version();
            let moving_pose = game.motion.poses.iter().any(|p| p.lift > 0);
            if !game.motion.poses.is_empty()
                && (frame == 20 || (!checked_lift && moving_pose) || frame == 150)
            {
                let engine = game.engine.as_ref().quiet();
                let view = game
                    .motion
                    .presenter
                    .view(game.presentation_ms())
                    .unwrap_or(View {
                        camera: (0, 0),
                        movers: Vec::new(),
                    });
                let mut fb = engine.frame().clone();
                fb.enable_src();
                u6_core::gfx::motion::paint_footsteps(
                    &engine.state(),
                    &game.motion.presenter,
                    &view,
                    &game.motion.poses,
                    &mut fb,
                );
                let pixels = u6_render::motion::LayerPixels {
                    x: 0,
                    y: 0,
                    width: u6_render::WIDTH,
                    height: u6_render::HEIGHT,
                    pixels: fb.pixels().to_vec(),
                    src: fb.src().quiet().to_vec(),
                    drawn: vec![true; u6_render::WIDTH * u6_render::HEIGHT],
                };
                let mut expected = Vec::new();
                game.hd.compose_layer(engine, &pixels, &mut expected);
                let actual = page.draw(&game, &game.slide());
                assert!(
                    map_words(&game, &actual).0 == map_words(&game, &expected).0,
                    "HD layers and full classic plan agree with real replacement art"
                );
                checked_lift |= moving_pose;
                let cached = (game.slide_layers_version(), game.frame_version());
                for _ in 0..4 {
                    game.present();
                    page.take(&mut game);
                }
                assert_eq!(
                    (game.slide_layers_version(), game.frame_version()),
                    cached,
                    "unchanged source/generation/pose reuses the cache"
                );
            }
        }
        assert!(checked_lift && replaced > 0);
        assert_eq!(page.misses, 0);
    }

    fn attack_fixture(ranged: bool) -> Option<(Game, Rc<std::cell::RefCell<u6_core::State>>)> {
        let (mut game, state) = castle_with_state()?;
        game.set_motion(true);
        game.set_hd(true, 2);
        {
            let mut s = state.borrow_mut();
            for a in 0..256 {
                if s.actors.attrs.status[a] & 0x80 == 0 {
                    s.actors.attrs.status[a] |= 0x10;
                }
            }
            let mut item = u6_core::objects::first_child(&mut s.objects, 1);
            while item >= 0 {
                s.objects.obj_status[item as usize] &= !0x08;
                item = u6_core::objects::next_child(&mut s.objects);
            }
            assert!(
                u6_core::objects::create_in(
                    &mut s,
                    if ranged { 0x29 } else { 0x22 },
                    0x18,
                    0xff00,
                    1
                ) >= 0
            );
        }
        if ranged {
            assert!(u6_core::objects::create_in(&mut state.borrow_mut(), 0x37, 0x10, 10, 1) >= 0);
        }
        Some((game, state))
    }

    /// Optional host fixture from the owner's archive. Never writes beneath a Git checkout.
    #[test]
    #[ignore = "writes private runtime attack saves outside the repository"]
    #[allow(clippy::disallowed_methods)]
    fn write_runtime_attack_fixtures() {
        let root = std::path::PathBuf::from(
            std::env::var_os("U6_ATTACK_FIXTURE").expect("private output required"),
        );
        std::fs::create_dir_all(&root).quiet();
        let root = std::fs::canonicalize(root).quiet();
        assert!(root.ancestors().all(|p| !p.join(".git").exists()));
        for ranged in [false, true] {
            let (_, state) = attack_fixture(ranged).expect("original archive required");
            let mut state = state.borrow_mut();
            u6_core::saveload::save(&mut state);
            let files = whole_save(&state, 0);
            assert!(!files.is_empty());
            for (name, bytes) in files {
                let path = root
                    .join(if ranged { "ranged" } else { "melee" })
                    .join(name);
                std::fs::create_dir_all(path.parent().quiet()).quiet();
                std::fs::write(path, bytes).quiet();
            }
        }
    }

    #[test]
    fn attacks_from_the_real_command_lunge_and_recoil_without_changing_the_engine() {
        for (ranged, suspended) in [(false, false), (true, false), (false, true), (true, true)] {
            let Some((mut game, state)) = attack_fixture(ranged) else {
                return;
            };
            let mut page = Page::new();
            for _ in 0..40 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
            }
            game.key_down("KeyA", "a", false, game.clock);
            game.key_up("KeyA");
            for _ in 0..200 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
                if state.borrow().ui.input.targeting != 0 {
                    break;
                }
            }
            assert_ne!(
                state.borrow().ui.input.targeting,
                0,
                "actual Attack target prompt"
            );
            down(&mut game, "ArrowDown");
            game.key_up("ArrowDown");
            for _ in 0..10 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
            }
            down(&mut game, "Enter");
            game.key_up("Enter");
            game.set_attack_suspended(suspended);
            let mut offsets = Vec::new();
            let mut changed_pixels = false;
            let mut zero_step_changes = 0;
            let mut previous_offsets = Vec::new();
            for _ in 0..360 {
                let before = game.engine.as_ref().quiet().io().now_ms();
                frame(&mut game, 1000.0 / 120.0, &mut |_| {});
                let stepped = before != game.engine.as_ref().quiet().io().now_ms();
                if stepped || game.sliding() {
                    game.present();
                    page.take(&mut game);
                }
                let now: Vec<_> = game
                    .motion
                    .poses
                    .iter()
                    .map(|p| (p.slot, p.offset))
                    .collect();
                if !stepped && now != previous_offsets && now.iter().any(|p| p.1 != (0, 0)) {
                    zero_step_changes += 1;
                }
                previous_offsets = now;
                if let Some(pose) = game
                    .motion
                    .poses
                    .iter()
                    .find(|p| p.slot == 1 && p.offset != (0, 0))
                {
                    offsets.push(pose.offset);
                    let e = game.engine.as_ref().quiet();
                    let state_before = e.state().clone();
                    let base = e.published_frame().pixels().to_vec();
                    let mut fb = e.frame().clone();
                    fb.enable_src();
                    let view = View {
                        camera: (0, 0),
                        movers: Vec::new(),
                    };
                    u6_core::gfx::motion::paint_footsteps(
                        &e.state(),
                        &game.motion.presenter,
                        &view,
                        &game.motion.poses,
                        &mut fb,
                    );
                    let mut neutral = game.motion.poses.clone();
                    for p in &mut neutral {
                        p.offset = (0, 0);
                    }
                    let mut old = e.frame().clone();
                    u6_core::gfx::motion::paint_footsteps(
                        &e.state(),
                        &game.motion.presenter,
                        &view,
                        &neutral,
                        &mut old,
                    );
                    let pixels = u6_render::motion::LayerPixels {
                        x: 0,
                        y: 0,
                        width: u6_render::WIDTH,
                        height: u6_render::HEIGHT,
                        pixels: fb.pixels().to_vec(),
                        src: fb.src().quiet().to_vec(),
                        drawn: vec![true; u6_render::WIDTH * u6_render::HEIGHT],
                    };
                    let mut expected = Vec::new();
                    game.hd.compose_layer(e, &pixels, &mut expected);
                    let actual = page.draw(&game, &game.slide());
                    assert!(
                        map_words(&game, &actual).0 == map_words(&game, &expected).0,
                        "attack HD layers agree with complete classic raster"
                    );
                    changed_pixels |= fb.pixels() != old.pixels();
                    assert!(*e.state() == state_before);
                    assert!(e.published_frame().pixels() == base);
                }
            }
            if suspended {
                assert!(
                    offsets.is_empty(),
                    "future cues while Settings is open stay suppressed"
                );
                game.set_attack_suspended(false);
                game.present();
                assert!(
                    game.attack_offsets().is_empty(),
                    "closing cannot replay old cues"
                );
                continue;
            }
            assert!(
                !offsets.is_empty(),
                "the actual physical attack produced visible poses (ranged={ranged})"
            );
            assert!(
                offsets
                    .iter()
                    .all(|&(x, y)| x == 0 && if ranged { y < 0 } else { y > 0 })
            );
            assert!(changed_pixels, "actual avatar pixels move");
            assert!(
                zero_step_changes > 0,
                "attack advances on 120 Hz no-step frames through actual app render gate"
            );
            assert!(
                game.motion.poses.iter().all(|p| p.offset == (0, 0)),
                "returns to rest"
            );
        }
    }

    #[test]
    fn attacks_end_on_modal_max_foreign_and_implicit_scene_restarts() {
        use u6_render::motion::attacks::Strike;
        let strike = Strike {
            slot: 1,
            direction: (1, 0),
            ranged: false,
        };
        for kind in 0..6 {
            let mut m = Motion::<Scene>::new();
            m.publish(
                [(Some(Rc::new(walker_scene(0))), 0.0, false)],
                Some(1.0),
                false,
                0.0,
            );
            m.attacks.start(strike, 1.0, 1);
            assert!(m.attacks.active(20.0));
            let mut scene = walker_scene(if kind == 2 { 20 } else { 0 });
            match kind {
                1 => scene.snap = true,
                3 => scene.leader = Some(2),
                4 => scene.centre.2 = 1,
                _ => {}
            }
            if kind == 5 {
                m.publish([], None, false, 20.0);
            } else {
                m.publish(
                    [(
                        if kind == 0 {
                            None
                        } else {
                            Some(Rc::new(scene))
                        },
                        20.0,
                        false,
                    )],
                    Some(1.0),
                    false,
                    20.0,
                );
            }
            assert!(!m.attacks.active(20.0), "barrier case {kind}");
            m.publish(
                [(Some(Rc::new(walker_scene(0))), 40.0, false)],
                Some(1.0),
                false,
                40.0,
            );
            assert!(
                !m.attacks.active(40.0),
                "returning to the world cannot revive it"
            );
        }
        let mut game = Game::new();
        game.motion.attacks.start(strike, 0.0, 1);
        assert!(game.sliding());
        game.release_keys(); // Settings/chooser and focus loss use this actual entry point.
        assert!(!game.motion.attacks.active(10.0));
    }

    #[test]
    fn footsteps_change_display_only_and_off_restores_the_original_frame() {
        let Some((mut game, state)) = castle_with_state() else {
            return;
        };
        let Some((mut control, original)) = castle_with_state() else {
            return;
        };
        for shared in [&state, &original] {
            let mut s = shared.borrow_mut();
            for n in 0..256 {
                if s.actors.attrs.status[n] & 0x80 == 0 {
                    s.actors.attrs.status[n] |= 0x10;
                }
            }
        }
        game.set_motion(true);
        control.set_motion(true);
        control.set_footstep_pattern(0);
        assert_eq!(game.footstep_pattern(), 2);
        let (mut page, mut plain) = (Page::new(), Page::new());
        let mut saw_lift = false;
        let mut saw_difference = false;
        let mut phases = std::collections::BTreeSet::new();
        let mut idle_words = std::collections::BTreeSet::new();
        for frame in 0..420 {
            if frame == 180 {
                assert!(down(&mut game, "ArrowDown"));
                assert!(down(&mut control, "ArrowDown"));
            }
            if frame == 280 {
                assert!(game.key_up("ArrowDown"));
                assert!(control.key_up("ArrowDown"));
            }
            page_frame_at(&mut game, &mut page, &mut |_| {});
            page_frame_at(&mut control, &mut plain, &mut |_| {});
            assert!(
                *state.borrow() == *original.borrow(),
                "gait never changes State or RNG"
            );
            assert!(
                game.engine.as_ref().quiet().published_frame().pixels()
                    == control.engine.as_ref().quiet().published_frame().pixels(),
                "engine publications are unchanged"
            );
            assert!(!game.old_cell());
            if frame < 180 {
                idle_words.insert(state.borrow().objects.type_frame[1]);
                assert!(
                    game.motion
                        .poses
                        .iter()
                        .all(|p| p.phase == 1 && p.lift == 0)
                );
            }
            if frame > 180 && frame < 280 {
                let s = state.borrow();
                for &slot in &s.actors.party.members[..s.actors.party.count()] {
                    let word = s.objects.type_frame[usize::from(slot)];
                    let kind = word & 0x3ff;
                    if ((0x178..=0x183).contains(&kind) || (0x199..=0x19a).contains(&kind))
                        && word >> 10 & 3 != 3
                    {
                        assert!(
                            game.motion.poses.iter().any(|p| p.slot == u16::from(slot)),
                            "each actual standing human party member has a pose"
                        );
                    }
                }
            }
            saw_difference |= game.frame_rgba() != control.frame_rgba();
            for p in &game.motion.poses {
                saw_lift |= p.lift > 0;
                assert!((0..=2).contains(&p.lift));
                phases.insert(p.phase);
            }
        }
        assert!(
            idle_words.len() > 1,
            "the original idle RNG still changes actor frames"
        );
        assert!(
            saw_difference && saw_lift,
            "actual displayed art changes and bobs during travel"
        );
        assert_eq!(phases, [0, 1, 2].into_iter().collect());
        assert!(
            !game.sliding(),
            "gait at rest is not an active movement track"
        );
        assert!(
            game.motion.poses.iter().any(|p| p.slot == 1),
            "settled avatar retains an actual neutral pose"
        );
        assert!(
            game.motion
                .poses
                .iter()
                .all(|p| p.phase == 1 && p.lift == 0)
        );
        game.set_footstep_pattern(0);
        game.present();
        control.present();
        assert!(
            game.frame_rgba() == control.frame_rgba(),
            "Off restores the original display exactly"
        );
        assert!(*state.borrow() == *original.borrow());
    }

    #[test]
    fn a_pointer_walk_has_the_same_continuity_contract() {
        continuous_walk_receipt(None, &[(true, false)], ContinuousInput::Pointer);
    }

    #[test]
    fn a_travel_walk_has_the_same_continuity_contract() {
        continuous_walk_receipt(None, &[(true, false)], ContinuousInput::Travel);
    }

    #[test]
    fn a_menu_go_here_walk_has_the_same_continuity_contract() {
        continuous_walk_receipt(None, &[(true, false)], ContinuousInput::Menu);
    }

    /// Actual pointer moves can change the walking direction several times before the next
    /// command. Retarget each displayed mover from where it was drawn, and settle on release.
    #[test]
    fn a_pointer_walk_retargets_from_drawn_positions_and_releases_its_tracks() {
        let mut controls = Vec::new();
        for adaptive in [false, true] {
            let Some((mut game, state)) = castle_with_state() else {
                return;
            };
            {
                let mut s = state.borrow_mut();
                for n in 0..256 {
                    if s.actors.attrs.status[n] & 0x80 == 0 {
                        s.actors.attrs.status[n] |= 0x10;
                    }
                }
            }
            game.motion.cadence = adaptive;
            game.set_motion(true);
            game.set_speed(4.0);
            let (hx, hy) = hero_point();
            let mut page = Page::new();
            let mut previous = std::collections::BTreeMap::new();
            let mut moving = 0;
            let mut changes = 0;
            for frame in 0..320 {
                match frame {
                    0 | 82 => game.pointer(hx, hy + 64, 1),
                    80 | 100 => game.pointer(hx, hy - 64, 1),
                    81 => game.pointer(hx + 64, hy, 1),
                    140 => game.pointer(hx, hy - 64, 0),
                    _ => {}
                }
                page_frame_at(&mut game, &mut page, &mut |_| {});
                if adaptive && frame == 50 {
                    assert!(
                        game.motion.presenter.walking(),
                        "the accepted pointer walk owns a cadence session"
                    );
                }
                assert!(
                    !game.old_cell(),
                    "no old-cell picture on pointer retarget/release"
                );
                let t = game.presentation_ms();
                let Some(shot) = game.motion.presenter.history().first() else {
                    continue;
                };
                let view = game
                    .motion
                    .shown
                    .as_ref()
                    .filter(|_| game.motion.drawn)
                    .map(|v| &v.0);
                let timings = game.motion.presenter.timings();
                let distances = game.motion.presenter.path_distances();
                let mut positions = vec![(
                    -1,
                    (shot.scene.centre.0, shot.scene.centre.1),
                    view.map_or((0, 0), |v| v.camera),
                )];
                positions.extend(shot.scene.placed.iter().map(|p| {
                    (
                        i32::from(p.slot),
                        p.cell,
                        view.and_then(|v| v.movers.iter().find(|m| m.slot == p.slot))
                            .map_or((0, 0), |m| m.offset),
                    )
                }));
                for (slot, cell, offset) in positions {
                    let drawn = (
                        f64::from(cell.0 * 16 + offset.0),
                        f64::from(cell.1 * 16 + offset.1),
                    );
                    let velocity = timings.iter().find(|v| v.0 == slot).map_or(0.0, |v| {
                        distances.iter().find(|d| d.0 == slot).map_or(0.0, |d| d.1) / v.2.max(0.001)
                    });
                    if let Some((old, old_cell, pt, pv)) =
                        previous.insert(slot, (drawn, cell, t, velocity))
                    {
                        let d = (drawn.0 - old.0).abs().max((drawn.1 - old.1).abs());
                        moving += usize::from(slot == -1 && d > 0.0);
                        changes += usize::from(
                            slot == -1 && cell != old_cell && (80..140).contains(&frame),
                        );
                        if (cell.0 - old_cell.0).abs().max((cell.1 - old_cell.1).abs()) <= 1 {
                            let bound = (velocity.max(pv).max(16.0 / u6_render::motion::SLIDE_MS)
                                * (t - pt))
                                .ceil();
                            assert!(
                                d <= bound,
                                "pointer retarget/release slot {slot} frame {frame}: {d}px exceeds drawn distance/D bound {bound}px"
                            );
                        }
                    }
                }
            }
            assert!(
                moving > 20 && changes >= 2,
                "the pointer genuinely moves before and after retarget"
            );
            assert!(
                !game.motion.presenter.walking() && !game.sliding(),
                "release settles"
            );
            assert!(
                game.motion.presenter.timings().is_empty(),
                "release drains tracks"
            );
            assert!(
                game.motion.presenter.history().len() <= 1,
                "release drops obsolete scenes"
            );
            assert_eq!(game.motion.presenter.capacity_resets(), 0);
            controls.push(state.borrow().clone());
        }
        assert!(
            controls[0] == controls[1],
            "pointer presentation preserves the whole authoritative State"
        );
    }

    #[test]
    fn a_stopped_travel_settles_without_changing_the_rules() {
        continuous_walk_receipt(None, &[(true, false)], ContinuousInput::Stop);
    }

    #[test]
    fn a_finite_speed_change_keeps_the_actual_walk_display_clock_and_positions() {
        for input in [
            ContinuousInput::Keyboard,
            ContinuousInput::Pointer,
            ContinuousInput::Travel,
        ] {
            let Some((mut game, state)) = castle_with_state() else {
                return;
            };
            {
                let mut s = state.borrow_mut();
                for n in 0..256 {
                    if s.actors.attrs.status[n] & 0x80 == 0 {
                        s.actors.attrs.status[n] |= 0x10;
                    }
                }
            }
            game.set_motion(true);
            match input {
                ContinuousInput::Keyboard => assert!(down(&mut game, "ArrowDown")),
                ContinuousInput::Pointer => {
                    let (x, y) = hero_point();
                    game.pointer(x, y + 64, 1);
                }
                ContinuousInput::Travel => {
                    let p = game.leader_position();
                    assert!(game.travel_to(p[0], p[1] + 5));
                }
                _ => unreachable!(),
            }
            let mut page = Page::new();
            let mut found = false;
            for _ in 0..240 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
                if game.motion.drawn
                    && game.pacer.carried_ms() >= 10.0
                    && game.motion.shown.as_ref().is_some_and(|(v, _, _, _)| {
                        let distance = v.camera.0.abs().max(v.camera.1.abs());
                        (1..16).contains(&distance)
                    })
                {
                    found = true;
                    break;
                }
            }
            assert!(found, "{input:?}: a real carried frame is displayed");
            let before = game.motion.shown.as_ref().quiet().0.clone();
            let time = game.presentation_ms();
            let version = game.engine.as_ref().quiet().published_version();
            let unchanged = state.borrow().clone();
            game.set_speed(4.0);
            assert_eq!(game.pacer.carried_ms(), 0.0);
            assert!((game.engine.as_ref().quiet().io().now_ms() as f64) < time);
            assert_eq!(game.pace(0.0), 0, "no engine step is due");
            game.present();
            assert_eq!(game.presentation_ms(), time, "display clock cannot rewind");
            assert_eq!(
                game.motion.shown.as_ref().quiet().0,
                before,
                "{input:?}: every displayed mover stays where it was drawn"
            );
            assert_eq!(game.engine.as_ref().quiet().published_version(), version);
            assert!(*state.borrow() == unchanged, "State and RNG stay unchanged");
            assert!(!game.old_cell());
            assert_eq!(game.motion.presenter.capacity_resets(), 0);
        }
    }

    #[test]
    fn max_and_instant_cancel_running_motion_without_an_engine_publication() {
        for instant in [false, true] {
            let Some(mut game) = in_the_castle() else {
                return;
            };
            game.set_motion(true);
            let mut page = Page::new();
            assert!(down(&mut game, "ArrowDown"));
            for _ in 0..120 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
                if game.sliding() {
                    break;
                }
            }
            assert!(
                game.sliding(),
                "a real slide is running before the mode changes"
            );
            let version = game.engine.as_ref().quiet().published_version();
            if instant {
                game.set_turn_speed(0);
            } else {
                game.set_speed(0.0);
            }
            game.pace(FRAME);
            game.present();
            assert_eq!(
                game.engine.as_ref().quiet().published_version(),
                version,
                "no engine publication"
            );
            assert!(
                !game.sliding() && !game.motion.drawn && game.slide().is_empty(),
                "Max/Instant clears the displayed slide immediately"
            );
            assert!(
                game.motion.presenter.timings().is_empty(),
                "Max/Instant leaves no running tracks"
            );
        }
    }

    #[test]
    fn hero_menu_and_ctrl_ally_presses_never_start_a_walking_session() {
        for gesture in 0..3 {
            let Some(mut game) = in_the_castle() else {
                return;
            };
            game.engine.as_mut().quiet().set_context_menu(true);
            game.set_motion(true);
            let (hx, hy) = hero_point();
            let point = if gesture == 2 {
                let e = game.engine.as_ref().quiet();
                let s = e.state();
                let member = s.objects.location[usize::from(s.actors.party.members[1])];
                (
                    hx + (i32::from(member.x) - i32::from(s.map.centre.x)) * 16,
                    hy + (i32::from(member.y) - i32::from(s.map.centre.y)) * 16,
                )
            } else {
                (hx, hy)
            };
            if gesture == 2 {
                game.set_control_action(true);
                game.set_modifiers(true);
            }
            game.pointer(point.0, point.1, 0);
            let mut page = Page::new();
            for _ in 0..4 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
            }
            if gesture == 2 {
                assert!(
                    game.engine.as_ref().quiet().quick_action().is_some(),
                    "Ctrl has the actual ally action preview"
                );
            }
            game.pointer(point.0, point.1, if gesture == 1 { 2 } else { 1 });
            for _ in 0..45 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
                assert!(
                    !game.motion.presenter.walking(),
                    "hero pass, menu or consumed Ctrl ally press is not a world walk"
                );
            }
            if gesture == 1 {
                assert!(drawn(&game).is_some(), "the actual menu opened");
            }
            if gesture == 2 {
                assert_eq!(game.game_buttons(), 0, "Ctrl captured the press");
            }
            game.pointer(point.0, point.1, 0);
            for _ in 0..30 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
            }
            assert!(
                !game.motion.presenter.walking(),
                "no session survives the release"
            );
        }
    }

    #[test]
    fn a_detouring_travel_keeps_continuity_through_route_corners() {
        continuous_walk_receipt(None, &[(true, false)], ContinuousInput::Detour);
    }

    /// Find a natural castle route with at least two corners using Travel itself on copies.
    /// The receipt then runs that route through the real engine, with unchanged movement points.
    fn detour_destination(state: &u6_core::State) -> (u16, u16) {
        use u6_core::actors::travel::{Next, Polled, Travel};
        let leader = state.actors.party.leader().quiet().index();
        let origin = state.objects.location[leader];
        for dy in -5i16..=5 {
            for dx in -5i16..=5 {
                if dx.abs().max(dy.abs()) < 4 {
                    continue;
                }
                let target = (
                    origin.x.wrapping_add_signed(dx),
                    origin.y.wrapping_add_signed(dy),
                );
                let Ok(mut route) = Travel::start(state, target) else {
                    continue;
                };
                let mut probe = state.clone();
                let mut directions = Vec::new();
                for _ in 0..24 {
                    let next = route.next(
                        &probe,
                        &Polled {
                            input: &[],
                            buttons: 0,
                            queued: false,
                            foreign: false,
                            menu: false,
                            due: true,
                        },
                    );
                    let Next::Press(key) = next else {
                        break;
                    };
                    let (x, y) = match key {
                        Key::Up => (0, -1),
                        Key::UpRight => (1, -1),
                        Key::Right => (1, 0),
                        Key::DownRight => (1, 1),
                        Key::Down => (0, 1),
                        Key::DownLeft => (-1, 1),
                        Key::Left => (-1, 0),
                        Key::UpLeft => (-1, -1),
                        _ => unreachable!("a travel only emits movement"),
                    };
                    directions.push((x, y));
                    let at = &mut probe.objects.location[leader];
                    at.x = at.x.wrapping_add_signed(x);
                    at.y = at.y.wrapping_add_signed(y);
                    probe.map.centre = *at;
                }
                if directions.len() >= 5
                    && directions.windows(2).filter(|w| w[0] != w[1]).count() >= 2
                {
                    return target;
                }
            }
        }
        panic!("the quiet castle has no route with two corners");
    }

    fn held_walk_receipt(locale: Option<&LocalePack>, cases: &[(bool, bool)]) {
        continuous_walk_receipt(locale, cases, ContinuousInput::Keyboard);
    }

    fn continuous_walk_receipt(
        locale: Option<&LocalePack>,
        cases: &[(bool, bool)],
        input: ContinuousInput,
    ) {
        let mut quiet_dwell = Vec::new();
        for &(quiet, safe) in cases {
            for speed in [1.0, 4.0, 8.0] {
                for turn in [1, 4] {
                    let mut controls = Vec::new();
                    for adaptive in [false, true] {
                        let Some((mut game, state)) = castle_with_state() else {
                            return;
                        };
                        if let Some(pack) = locale {
                            assert!(game.set_locale(pack).is_empty(), "owned locale installs");
                        }
                        if quiet {
                            let mut s = state.borrow_mut();
                            for n in 0..256 {
                                if s.actors.attrs.status[n] & 0x80 == 0 {
                                    s.actors.attrs.status[n] |= 0x10;
                                }
                            }
                        }
                        if safe {
                            state.borrow_mut().actors.movement_points[..].fill(127);
                        }
                        game.motion.cadence = adaptive;
                        game.set_motion(true);
                        game.set_speed(speed);
                        game.set_turn_speed(turn);
                        let (hx, hy) = hero_point();
                        let pointer = (hx, hy + 64);
                        let origin = game.leader_position();
                        let destination = if input == ContinuousInput::Detour {
                            detour_destination(&state.borrow())
                        } else {
                            (origin[0], origin[1] + 5)
                        };
                        match input {
                            ContinuousInput::Keyboard => assert!(down(&mut game, "ArrowDown")),
                            ContinuousInput::Pointer => game.pointer(pointer.0, pointer.1, 1),
                            ContinuousInput::Travel
                            | ContinuousInput::Stop
                            | ContinuousInput::Detour => {
                                assert!(game.travel_to(destination.0, destination.1));
                            }
                            ContinuousInput::Menu => {
                                game.engine.as_mut().quiet().set_context_menu(true);
                                let cell = cell_point(
                                    u6_core::geom::HALF_W as i32,
                                    u6_core::geom::HALF_H as i32 + 5,
                                );
                                click(&mut game, cell, 2);
                                let view = drawn(&game).expect("the menu opens on the destination");
                                let row = row_of(&view, u6_core::ui::menu::GO_HERE)
                                    .expect("Go here is offered");
                                assert!(view.rows[row].enabled(), "the destination is reachable");
                                click(&mut game, row_point(&view, row), 1);
                                assert_eq!(game.go_here_asked(), 1, "the real menu starts travel");
                                assert!(game.travel_active());
                            }
                        }
                        let start = game.clock;
                        let mut page = Page::new();
                        let party_slots: Vec<i32> = {
                            let s = state.borrow();
                            s.actors.party.members[..s.actors.party.count()]
                                .iter()
                                .map(|&v| i32::from(v))
                                .collect()
                        };
                        let mut mover_frames: std::collections::BTreeMap<i32, (usize, usize, f64)> =
                            std::collections::BTreeMap::new();
                        let mut at = game.leader_position();
                        let mut steps = 0;
                        let mut gaps = Vec::new();
                        let mut last_step = None;
                        let mut previous: std::collections::BTreeMap<i32, (f64, f64, f64, f64)> =
                            std::collections::BTreeMap::new();
                        let mut targets: std::collections::BTreeMap<i32, Vec<(f64, f64)>> =
                            std::collections::BTreeMap::new();
                        let mut logical: std::collections::BTreeMap<i32, (f64, f64)> =
                            std::collections::BTreeMap::new();
                        let mut own_steps: std::collections::BTreeMap<i32, usize> =
                            std::collections::BTreeMap::new();
                        let mut first_snaps: std::collections::BTreeMap<i32, usize> =
                            std::collections::BTreeMap::new();
                        {
                            let s = state.borrow();
                            logical.insert(
                                -1,
                                (
                                    f64::from(s.map.centre.x) * 16.0,
                                    f64::from(s.map.centre.y) * 16.0,
                                ),
                            );
                            for &slot in &party_slots {
                                let p = s.objects.location[slot as usize];
                                logical
                                    .insert(slot, (f64::from(p.x) * 16.0, f64::from(p.y) * 16.0));
                            }
                        }
                        let (
                            mut moving,
                            mut stopped,
                            mut old,
                            mut jump,
                            mut passed,
                            mut retargeted,
                        ) = (0, 0, 0, 0.0f64, 0, 0);
                        let (mut stop_run, mut longest_stop, mut largest_lag) = (0, 0, 0.0f64);
                        let (mut max_history, mut max_static_pixels) = (0, 0);
                        let mut kept_frames = 0;
                        let mut cancelled = 0;
                        let mut previous_direction = None;
                        let mut corners = 0;
                        let mut held = true;
                        let mut settle_velocity = 0.0f64;
                        let mut tail = 0;
                        let mut drained = 0;
                        for _ in 0..2400 {
                            let travelling = matches!(
                                input,
                                ContinuousInput::Travel
                                    | ContinuousInput::Menu
                                    | ContinuousInput::Stop
                                    | ContinuousInput::Detour
                            );
                            if held
                                && if input == ContinuousInput::Stop {
                                    steps >= 3
                                } else if travelling {
                                    !game.travel_active()
                                } else {
                                    steps >= 7 || game.clock - start >= 6000.0
                                }
                            {
                                settle_velocity =
                                    16.0 * f64::from(
                                        game.engine.as_ref().quiet().io().playback_speed().code(),
                                    ) / u6_render::motion::SLIDE_MS;
                                match input {
                                    ContinuousInput::Keyboard => assert!(game.key_up("ArrowDown")),
                                    ContinuousInput::Pointer => {
                                        game.pointer(pointer.0, pointer.1, 0)
                                    }
                                    ContinuousInput::Stop => {
                                        assert!(down(&mut game, "Escape"));
                                        assert!(game.key_up("Escape"));
                                    }
                                    ContinuousInput::Travel
                                    | ContinuousInput::Menu
                                    | ContinuousInput::Detour => {
                                        assert_eq!(
                                            game.engine.as_ref().quiet().travel_end(),
                                            Some(u6_core::actors::travel::End::Arrived)
                                        );
                                        assert!(
                                            game.leader_position()[..2]
                                                == [destination.0, destination.1],
                                            "the real travel arrived"
                                        );
                                    }
                                }
                                held = false;
                            }
                            page_frame_at(&mut game, &mut page, &mut |_| {});
                            // Arrival is discovered inside this frame's engine tick, unlike a
                            // physical release queued before it. Its remaining tracks settle
                            // at the same base speed starting in this very frame.
                            if travelling && !game.travel_active() {
                                settle_velocity =
                                    16.0 * f64::from(
                                        game.engine.as_ref().quiet().io().playback_speed().code(),
                                    ) / u6_render::motion::SLIDE_MS;
                            }
                            let now = game.leader_position();
                            if now != at {
                                steps += 1;
                                at = now;
                                if let Some(t) = last_step.replace(game.clock) {
                                    gaps.push(game.clock - t);
                                }
                            }
                            let t = game.presentation_ms();
                            max_history = max_history.max(game.motion.presenter.history().len());
                            if let Some((_, _, w, h)) = game.motion.presenter.static_bounds() {
                                max_static_pixels = max_static_pixels.max(w * h);
                            }
                            let Some(shot) = game.motion.presenter.history().first() else {
                                continue;
                            };
                            let scene = &shot.scene;
                            let view = game
                                .motion
                                .shown
                                .as_ref()
                                .filter(|_| game.motion.drawn)
                                .map(|s| &s.0);
                            let timings = game.motion.presenter.timings();
                            let distances = game.motion.presenter.path_distances();
                            let mut positions = vec![(
                                -1,
                                f64::from(scene.centre.0 * 16),
                                f64::from(scene.centre.1 * 16),
                                view.map_or((0, 0), |v| v.camera),
                            )];
                            positions.extend(scene.placed.iter().map(|p| {
                                (
                                    i32::from(p.slot),
                                    f64::from(p.cell.0 * 16),
                                    f64::from(p.cell.1 * 16),
                                    view.and_then(|v| v.movers.iter().find(|m| m.slot == p.slot))
                                        .map_or((0, 0), |m| m.offset),
                                )
                            }));
                            for (slot, x, y, off) in positions
                                .into_iter()
                                .filter(|p| p.0 == -1 || party_slots.contains(&p.0))
                            {
                                let mut discontinuity = false;
                                let mut target_delta = (0.0, 0.0);
                                let drawn = (x + f64::from(off.0), y + f64::from(off.1));
                                let velocity =
                                    timings.iter().find(|v| v.0 == slot).map_or(0.0, |v| {
                                        distances.iter().find(|d| d.0 == slot).map_or(0.0, |d| d.1)
                                            / v.2.max(0.001)
                                    });
                                if let Some(old_target) = logical.insert(slot, (x, y))
                                    && old_target != (x, y)
                                {
                                    target_delta = (x - old_target.0, y - old_target.1);
                                    if slot == -1 {
                                        let direction =
                                            (target_delta.0.signum(), target_delta.1.signum());
                                        corners += usize::from(
                                            previous_direction
                                                .replace(direction)
                                                .is_some_and(|old| old != direction),
                                        );
                                    }
                                    *own_steps.entry(slot).or_default() += 1;
                                    discontinuity =
                                        (old_target.0 - x).abs().max((old_target.1 - y).abs())
                                            > 16.0;
                                    let pending = targets.entry(slot).or_default();
                                    if !pending.is_empty() {
                                        retargeted += 1;
                                    }
                                    // This fixture holds south in the open corridor. A sideways follower
                                    // formation change cancels its old directional targets explicitly.
                                    if input != ContinuousInput::Detour
                                        && (old_target.0 != x || old_target.1 > y)
                                    {
                                        cancelled += pending.len();
                                        pending.clear();
                                    }
                                    pending.push((x, y));
                                }
                                if let Some((px, py, pt, pv)) =
                                    previous.insert(slot, (drawn.0, drawn.1, t, velocity))
                                {
                                    let d = (drawn.0 - px).abs().max((drawn.1 - py).abs());
                                    if slot == -1 && held && steps >= 3 {
                                        moving += usize::from(d > 0.0);
                                        stopped += usize::from(d == 0.0);
                                        stop_run = if d == 0.0 { stop_run + 1 } else { 0 };
                                        longest_stop = longest_stop.max(stop_run);
                                        largest_lag = largest_lag
                                            .max(f64::from(off.0.abs().max(off.1.abs())));
                                    }
                                    let metric = mover_frames.entry(slot).or_default();
                                    metric.0 += usize::from(d > 0.0);
                                    metric.1 += usize::from(d == 0.0);
                                    metric.2 = metric.2.max(d);
                                    // Each party mover has its own distance/D bound. A logical multi-cell
                                    // change is an explicit teleport, not a continued track.
                                    if !discontinuity && !game.old_cell() {
                                        let bound = (velocity.max(pv).max(settle_velocity)
                                            * (t - pt))
                                            .ceil();
                                        let short_tap = u6_render::motion::SLIDE_MS
                                            / f64::from(turn)
                                            / game.pacer.pace_rate().unwrap_or(f64::INFINITY)
                                            < 2.0 * FRAME
                                            && (!adaptive
                                                || own_steps.get(&slot).copied().unwrap_or(0) <= 1);
                                        assert!(
                                            short_tap || d <= bound,
                                            "quiet {quiet} speed {speed} turn {turn} adaptive {adaptive} slot {slot}: jump {d} > actual distance/D bound {bound}; own steps {:?}, delta {target_delta:?}, previous velocity {pv}, current {velocity}, dt={}, held={held}, travel={}, offset={off:?}, drawn_delta=({}, {}), timing={:?}",
                                            own_steps.get(&slot),
                                            t - pt,
                                            game.travel_active(),
                                            drawn.0 - px,
                                            drawn.1 - py,
                                            timings.iter().find(|v| v.0 == slot),
                                        );
                                        if adaptive && short_tap && d > bound {
                                            assert_eq!(
                                                d, 16.0,
                                                "only the whole short first tap snaps"
                                            );
                                            assert_eq!(
                                                velocity, 0.0,
                                                "short tap has no active track"
                                            );
                                            *first_snaps.entry(slot).or_default() += 1;
                                        }
                                        jump = jump.max(d);
                                    }
                                    if let Some(pending) = targets.get_mut(&slot) {
                                        pending.retain(|&(tx, ty)| {
                                            let crossed = (px.min(drawn.0) - 0.5
                                                ..=px.max(drawn.0) + 0.5)
                                                .contains(&tx)
                                                && (py.min(drawn.1) - 0.5..=py.max(drawn.1) + 0.5)
                                                    .contains(&ty);
                                            passed += usize::from(crossed);
                                            !crossed
                                        });
                                    }
                                }
                            }
                            if adaptive
                                && quiet
                                && turn == 1
                                && steps == 3
                                && kept_frames < 3
                                && let Some(v) = view
                                && [4, 8, 12].contains(&v.camera.1.abs())
                            {
                                keep_frame(
                                    &format!(
                                        "w3-{}-{speed}-safe{safe}-ko{}-{}.png",
                                        u6_render::WIDTH,
                                        locale.is_some(),
                                        kept_frames
                                    ),
                                    (u6_render::WIDTH, u6_render::HEIGHT),
                                    &game.frame_rgba(),
                                );
                                kept_frames += 1;
                            }
                            old += usize::from(game.old_cell());
                            if !held {
                                tail = if game.sliding() { 0 } else { tail + 1 };
                                drained += 1;
                                if drained >= 180 {
                                    break;
                                }
                            }
                        }
                        assert!(
                            steps >= if input == ContinuousInput::Stop { 3 } else { 4 }
                                && tail >= 60,
                            "walk and final release rest"
                        );
                        if input == ContinuousInput::Stop {
                            assert_eq!(
                                game.engine.as_ref().quiet().travel_end(),
                                Some(u6_core::actors::travel::End::Input),
                                "Stop cancels the real travel"
                            );
                        }
                        if input == ContinuousInput::Detour {
                            assert!(
                                corners >= 2,
                                "the real engine travelled through at least two route corners"
                            );
                        }
                        assert_eq!(old, 0, "no old cell re-exposure");
                        assert!(
                            !game.motion.presenter.walking(),
                            "release/arrival ends cadence"
                        );
                        assert!(
                            game.motion.presenter.timings().is_empty(),
                            "release/arrival drains tracks"
                        );
                        assert_eq!(
                            game.motion.presenter.capacity_resets(),
                            0,
                            "ordinary cadence never exhausts the resource budget"
                        );
                        assert!(
                            first_snaps.values().all(|&n| n <= 1),
                            "each mover can snap only its own short first tap"
                        );
                        assert!(
                            targets.get(&-1).is_none_or(Vec::is_empty),
                            "every uncancelled camera target actually passed"
                        );
                        let s = state.borrow();
                        for &slot in &s.actors.party.members[..s.actors.party.count()] {
                            assert!(
                                targets.get(&i32::from(slot)).is_none_or(Vec::is_empty),
                                "every uncancelled party target actually passed"
                            );
                        }
                        let final_state = s.clone();
                        let gap: Vec<_> = gaps.iter().map(|v| v.round() as u64).collect();
                        println!(
                            "W3 input={input:?} quiet={quiet} safe={safe} rate={speed} O3={turn} adaptive={adaptive} korean={}: steps={steps}, gaps={gap:?}, camera moving={moving} stationary={stopped}, longest stationary={}ms, max lag={largest_lag}px, max jump={jump}px, passed={passed}, retargeted={retargeted}, direction-cancelled={cancelled}, final rest=true",
                            locale.is_some(),
                            longest_stop as f64 * FRAME
                        );
                        println!(
                            "W3 mover frames (slot, moving, stationary, max jump): {mover_frames:?}"
                        );
                        println!(
                            "W3 resources: history peak={max_history}, static source pixel peak={max_static_pixels}, capacity resets=0; short first taps={first_snaps:?}"
                        );
                        controls.push((final_state, moving, stopped, longest_stop));
                    }
                    // Presentation alone cannot alter the command stream or rules.
                    assert!(
                        controls[0].0 == controls[1].0,
                        "identical whole authoritative State at the same input/tick boundary"
                    );
                    if safe {
                        assert!(
                            controls[1].1 >= controls[0].1,
                            "quiet walk keeps every actual moving frame"
                        );
                        assert!(
                            controls[1].3 < controls[0].3,
                            "quiet walk removes the long per-cell dwell"
                        );
                        assert!(
                            controls[1].3 <= 3,
                            "quiet cadence has no per-cell dwell beyond pixel quantization/one tick"
                        );
                    }
                    if quiet && controls[1].3 > 3 {
                        quiet_dwell.push((speed as u32, turn, controls[1].3));
                    }
                }
            }
        }
        assert!(
            quiet_dwell.is_empty(),
            "ordinary quiet must remove per-cell dwell beyond pixel quantization/one tick; (speed,O3,stationary frames): {quiet_dwell:?}"
        );
    }

    /// Spec 20 with spec 23 (rounds 11–12): while the message history is scrolled back, a slide
    /// shows the history over the message window, not the live window. In the castle with smooth
    /// motion on at 1×, after a few passes: the down arrow tapped and, once the leader stepped (a
    /// key is the player's input and returns the view to the live bottom), the history wheeled
    /// three lines back through the engine's API while the step's slide runs. Then on every frame
    /// (the slide drawn off rest, the early picture held at rest with no slide placed nor drawn
    /// past it, and the first frame after the slide, which must come, with no slide placed and
    /// [`Game::sliding`] false) the page's frame off the shown map is the expected composition,
    /// made here from the published frame and the engine's view of the history alone
    /// ([`viewed_frame`], apart from the engine's composition and the page's output): with HD off
    /// the RGBA frame there is that frame with the pointer sprite over it; with HD on the frame the
    /// page draws ([`Page`]: the HD frame with the layers over its map, or the HD frame alone once
    /// no slide is placed) is its HD frame ([`hd_words_of`]). Every comparison first asserts the
    /// two frames' sizes equal. On each of them the engine's shown frame and the expected one
    /// differ from the published one off the shown map: the expected frames hold the history. In
    /// HD the three phases share one HD frame off the map (the slide's layers are clipped to the
    /// shown map), so HD's check is that frame's at each phase. (With a context menu open over the
    /// history no slide runs: [`a_context_menu_opened_during_a_slide_ends_it_and_it_does_not_resume`].)
    #[test]
    fn a_slide_keeps_the_message_history_the_player_scrolled_back_to() {
        use u6_core::geom::SHOWN;
        let off_map = |x: usize, y: usize| {
            let (x0, y0, x1, y1) = SHOWN;
            let (x, y) = (x as u16, y as u16);
            !((x0..=x1).contains(&x) && (y0..=y1).contains(&y))
        };
        // The pixels off the shown map where two frames of the classic size differ, of `bytes`
        // bytes a pixel (1: colour indices, 4: RGBA), the frames' sizes asserted equal first.
        let differ = |a: &[u8], b: &[u8], bytes: usize, what: &str| {
            let w = u6_render::WIDTH;
            assert_eq!(a.len(), b.len(), "{what}: the frames' sizes");
            assert_eq!(
                a.len(),
                w * u6_render::HEIGHT * bytes,
                "{what}: the classic size"
            );
            (0..a.len() / bytes)
                .filter(|&i| {
                    off_map(i % w, i / w)
                        && a[bytes * i..bytes * (i + 1)] != b[bytes * i..bytes * (i + 1)]
                })
                .count()
        };
        for hd in [false, true] {
            let label = if hd { "HD" } else { "classic" };
            let Some(mut game) = in_the_castle() else {
                return;
            };
            game.set_motion(true);
            if hd {
                game.set_hd(true, 2);
            }
            // The passes publish the scenes the step slides from.
            for _ in 0..6 {
                assert!(down(&mut game, "Space"));
                assert!(game.key_up("Space"));
                game.step(20);
                assert!(
                    steps_to_prompt(&mut game),
                    "{label}: the prompt after a pass"
                );
                game.present();
            }
            let mut page = Page::new();
            let at = game.leader_position();
            assert!(down(&mut game, "ArrowDown"));
            assert!(game.key_up("ArrowDown"));
            let mut wheeled = false;
            // Frames checked: off rest, held at rest, after the slide.
            let mut checked = [0usize; 3];
            for _ in 0..240 {
                page_frame_at(&mut game, &mut page, &mut |g| {
                    if !wheeled && g.leader_position() != at {
                        assert!(g.wheel(200, 150, -3), "{label}: over the message window");
                        wheeled = true;
                    }
                });
                if !wheeled {
                    continue;
                }
                assert_eq!(
                    game.history_offset(),
                    3,
                    "{label}: the history scrolled back"
                );
                // What the page shows now: the slide off rest (0), at rest (1), none (2).
                let phase = if hd {
                    match page.take(&mut game).then(|| static_offset(&game)).flatten() {
                        Some(off) => usize::from(off == (0, 0)),
                        None => 2,
                    }
                } else {
                    let shown = (game.motion.shown.as_ref()).filter(|_| game.motion.drawn);
                    shown.map_or(2, |s| usize::from(!off_rest(&s.0)))
                };
                if phase == 2 {
                    // Not a layer the page missed nor a slide not drawn while one runs (opus N1).
                    assert!(
                        game.slide().is_empty() && !game.sliding() && !game.motion.drawn,
                        "{label}: after the slide no slide placed, drawn or running"
                    );
                }
                let e = game.engine.as_ref().quiet();
                let live = e.published_frame().clone();
                let viewed = viewed_frame(e);
                // The published frame with the sprite over it: the shown frame less the view.
                let (mut live_shown, mut viewed_shown) = (live.clone(), viewed.clone());
                if let Some(sprite) = e.sprite() {
                    sprite.draw(&mut live_shown);
                    sprite.draw(&mut viewed_shown);
                }
                assert!(
                    differ(e.frame().pixels(), live_shown.pixels(), 1, label) > 0
                        && differ(viewed.pixels(), live.pixels(), 1, label) > 0,
                    "{label}: the shown and the expected frames hold the history"
                );
                let wrong = if hd {
                    let want = hd_words_of(e, &viewed);
                    let drawn = if phase < 2 {
                        page.draw(&game, &game.slide())
                    } else {
                        words_of(exported(game.hd_frame_ptr(), game.hd_frame_len()))
                    };
                    let ow = u6_render::WIDTH * 2;
                    assert_eq!(drawn.len(), want.len(), "{label}: the HD frames' sizes");
                    assert_eq!(
                        drawn.len(),
                        ow * u6_render::HEIGHT * 2,
                        "{label}: the HD size"
                    );
                    (drawn.iter().zip(&want).enumerate())
                        .filter(|&(i, (a, b))| off_map(i % ow / 2, i / ow / 2) && a != b)
                        .count()
                } else {
                    let mut want = Vec::new();
                    viewed_shown.write_rgba(&mut want);
                    differ(&game.frame_rgba(), &want, 4, label)
                };
                assert_eq!(
                    wrong, 0,
                    "{label}: off the shown map, the expected frame (phase {phase})"
                );
                checked[phase] += 1;
                if phase == 2 {
                    break;
                }
            }
            assert!(wheeled, "{label}: the step taken");
            assert!(
                checked[0] > 0 && checked[1] > 0 && checked[2] == 1,
                "{label}: frames off rest, held at rest, and the slide over: {checked:?}"
            );
            assert_eq!(
                game.history_offset(),
                3,
                "{label}: still scrolled back after the turn"
            );
            println!(
                "{label}: with the history scrolled back, {} frames drawing the slide off rest, \
                 {} holding the early picture at rest and the first after the slide, each the \
                 expected frame off the shown map",
                checked[0], checked[1]
            );
        }
    }

    /// `fb` composed as an HD frame at 2× by a compositor and image cache of its own (no pack),
    /// apart from the game's: its words.
    fn hd_words_of(e: &Engine, fb: &Framebuffer) -> Vec<u32> {
        let mut compositor = u6_render::hd::Compositor::new();
        let mut cache = u6_core::hd::ImageCache::new();
        let state = e.state();
        let mut images = cache.images(&state, e.io());
        compositor.compose(fb, &mut images, Scale::X2);
        compositor.rgba().to_vec()
    }

    /// One 60 Hz display frame at the game's speed as the page runs it (site/app.js `frame`): the
    /// clock moved on, the steps [`Game::pace`] gives, each followed by `each`, the present after
    /// a frame that stepped or while sliding, then the page's take of the slide's layers
    /// ([`Page::take`]; nothing with HD off). The motion receipts' one frame (round 13, opus N8):
    /// spec 19's [`frame`] presents nothing, and spec 17's [`page_frame`] runs on the page's clock
    /// given whole with Max's budget.
    fn page_frame_at(game: &mut Game, page: &mut Page, each: &mut impl FnMut(&mut Game)) {
        game.set_clock(game.clock + FRAME);
        let k = game.pace(FRAME);
        for _ in 0..k {
            game.step(20);
            each(game);
        }
        game.ran(k);
        if k > 0 || game.sliding() {
            game.present();
        }
        page.take(game);
    }

    /// The directory `U6_FRAMES_DIR` names, made, for frames the dispatcher reads; none without it.
    /// Fails closed on a path that is not absolute, holds `.` or `..`, or lies (resolved) in the
    /// worktree or the main checkout it lies in.
    #[allow(clippy::disallowed_methods)] // tests may read the environment and write files
    fn frames_dir() -> Option<std::path::PathBuf> {
        use std::path::{Component, Path, PathBuf};
        let dir = PathBuf::from(std::env::var_os("U6_FRAMES_DIR")?);
        assert!(
            dir.is_absolute()
                && (dir.components())
                    .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
            "U6_FRAMES_DIR an absolute path without . or .."
        );
        std::fs::create_dir_all(&dir).quiet();
        let made = std::fs::canonicalize(&dir).quiet();
        let repo =
            std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).quiet();
        let main = (repo.parent())
            .filter(|p| p.file_name().is_some_and(|n| n == ".worktrees"))
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        for root in std::iter::once(&repo).chain(main.as_ref()) {
            assert!(
                !made.starts_with(root),
                "the frames directory outside the repository"
            );
        }
        Some(made)
    }

    /// Writes `rgba` (`w` × `h` RGBA8 pixels) as `name`, a PNG (RGB, zlib stored blocks), into
    /// [`frames_dir`]; whether it did.
    #[allow(clippy::disallowed_methods)] // tests may write files
    fn keep_frame(name: &str, (w, h): (usize, usize), rgba: &[u8]) -> bool {
        fn crc32(data: &[u8]) -> u32 {
            let mut c = !0u32;
            for &b in data {
                c ^= u32::from(b);
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        0xEDB8_8320 ^ (c >> 1)
                    } else {
                        c >> 1
                    };
                }
            }
            !c
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
        }
        let Some(dir) = frames_dir() else {
            return false;
        };
        assert_eq!(rgba.len(), w * h * 4, "{name}: the frame's size");
        let mut raw = Vec::with_capacity(h * (w * 3 + 1));
        for row in rgba.chunks_exact(w * 4) {
            raw.push(0);
            for px in row.chunks_exact(4) {
                raw.extend_from_slice(&px[..3]);
            }
        }
        let mut z = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = raw.chunks(0xffff).collect();
        for (i, b) in blocks.iter().enumerate() {
            z.push(u8::from(i + 1 == blocks.len()));
            let n = b.len() as u16;
            z.extend_from_slice(&n.to_le_bytes());
            z.extend_from_slice(&(!n).to_le_bytes());
            z.extend_from_slice(b);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &x in &raw {
            a = (a + u32::from(x)) % 65521;
            b = (b + a) % 65521;
        }
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(w as u32).to_be_bytes());
        ihdr.extend_from_slice(&(h as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        std::fs::write(dir.join(name), out).quiet();
        true
    }

    /// The camera's offset the page shows now: with HD off the view the RGBA frame shows
    /// ([`Game::present_slide`]'s), with HD on the static layer's placement ([`static_offset`]);
    /// `None` for the engine's frame.
    fn shown_offset(game: &Game) -> Option<(i32, i32)> {
        if game.hd_on() {
            // The page draws the static layer where the world shows: at -camera.
            static_offset(game).map(|(x, y)| (-x, -y))
        } else {
            (game.motion.shown.as_ref())
                .filter(|_| game.motion.drawn)
                .map(|s| s.0.camera)
        }
    }

    /// Spec 20 (round 13, opus S3): one step tapped at the prompt (down, right, up, left, a pass
    /// after a round of them walked none), counted by [`Progress`] over its frames
    /// ([`shown_offset`]) until the prompt is back and nothing slides for 30 frames (at most 15 s),
    /// then closed; `None` when no key walked in four rounds.
    fn tap_step(game: &mut Game, page: &mut Page) -> Option<Progress> {
        for round in 0..4 {
            for key in ["ArrowDown", "ArrowRight", "ArrowUp", "ArrowLeft", "Space"] {
                if key == "Space" && round == 0 {
                    continue;
                }
                let at = game.leader_position();
                let takes = game.engine.as_ref().quiet().prompt_takes();
                assert!(down(game, key));
                assert!(game.key_up(key));
                let (mut step, mut still) = (None::<Progress>, 0);
                for _ in 0..900 {
                    let mut moved = None;
                    page_frame_at(game, page, &mut |g| {
                        let now = g.leader_position();
                        if moved.is_none() && now != at {
                            let d = |i: usize| i32::from(now[i]) - i32::from(at[i]);
                            moved = Some((d(0), d(1)));
                        }
                    });
                    if let Some(dir) = moved.filter(|_| step.is_none()) {
                        step = Some(Progress::new(dir));
                    }
                    if let Some(p) = step.as_mut() {
                        p.frame(shown_offset(game), game.old_cell(), true);
                    }
                    let taken = game.engine.as_ref().quiet().prompt_takes() > takes;
                    still = if taken && game.at_prompt() && !game.sliding() {
                        still + 1
                    } else {
                        0
                    };
                    if still >= 30 {
                        break;
                    }
                }
                if let Some(mut p) = step {
                    p.close();
                    return Some(p);
                }
            }
        }
        None
    }

    /// Spec 20 with spec 17 (round 12): an open context menu ends a running slide at once, so the
    /// menu is never drawn over a moving map nor a moving map under it. At 1×, HD off and on, the
    /// menu on as the page puts it on: in the page's start room ([`in_the_start_room`]) with the
    /// message history at its live bottom, and in its start fight ([`in_the_start_fight`]: combat
    /// mode, where the party slides from the turn's picture) with the history scrolled three lines
    /// back. Keys tapped (a step down, a pass, a step up, a pass, by turns) until the prompt comes
    /// back after one, the menu's gate open, with a slide still running for 100 ms or more (an
    /// object the turn moved still sliding there: an NPC out of the fight, a member in it; in the
    /// fight three history lines kept or more); the history wheeled back then, in the fight; then
    /// a right press on the hero. The first frame with the menu
    /// open shows no slide: the presenter started again (its generation moved: a snap from the
    /// menu is no publication), nothing of a slide drawn or placed, [`Game::sliding`] false. Its
    /// frames hold each layer where the one composition puts it (published, view, menu, sprite),
    /// against a frame made here from the published frame and the history's view alone
    /// ([`viewed_frame`]): the screen frame is that frame outside the menu's box and differs from
    /// it in most of the box; the shown frame is that frame with the box's pixels as the screen
    /// frame has them and the pointer sprite over it; the page shows it (HD off: the RGBA frame;
    /// HD on: no layer placed and the HD frame that frame's, composed here apart from the game's);
    /// the history stays scrolled back. While the menu stays open (its press let go) and after
    /// Escape closed it, through the slide's own end and 200 ms past it, no slide resumes: the
    /// generation stays and nothing is drawn or placed.
    #[test]
    fn a_context_menu_opened_during_a_slide_ends_it_and_it_does_not_resume() {
        for (hd, scrolled) in [(false, false), (true, false), (false, true), (true, true)] {
            let label = format!(
                "{}{}",
                if hd { "HD" } else { "classic" },
                if scrolled { ", history back" } else { "" }
            );
            let fixture = if scrolled {
                in_the_start_fight(hd)
            } else {
                in_the_start_room(hd)
            };
            let Some(mut game) = fixture else {
                return;
            };
            // This oracle requires no rest layers; B neutral restoration is checked separately.
            game.set_footstep_pattern(0);
            let mut page = Page::new();
            // Keys tapped (a step down, a pass, a step up, a pass, by turns) until the prompt is
            // back after one, the menu's gate open, with a slide running on (and, to wheel the
            // history back over, three lines or more kept).
            let (mut taps, mut found) = (0, false);
            'taps: while taps < 48 {
                let key = ["ArrowDown", "Space", "ArrowUp", "Space"][taps % 4];
                taps += 1;
                let takes = game.engine.as_ref().quiet().prompt_takes();
                assert!(down(&mut game, key));
                assert!(game.key_up(key));
                for _ in 0..900 {
                    page_frame_at(&mut game, &mut page, &mut |_| {});
                    let e = game.engine.as_ref().quiet();
                    let taken = e.prompt_takes() > takes;
                    if !(taken && game.pending.is_empty() && e.context_menu_gate(&[])) {
                        continue;
                    }
                    let kept = e.state().ui.scrollback.lines().len();
                    let (p, t) = (&game.motion.presenter, game.presentation_ms());
                    let running = p.view(t).filter(|_| p.active(t + 100.0));
                    if running.is_some_and(|v| off_rest(&v)) && (!scrolled || kept >= 3) {
                        found = true;
                        break 'taps;
                    }
                    break;
                }
            }
            assert!(
                found,
                "{label}: a prompt with a slide running ({taps} keys tapped)"
            );
            let back = if scrolled {
                assert!(game.wheel(200, 150, -3), "{label}: over the message window");
                3
            } else {
                0
            };
            let before = game.motion.presenter.generation();
            let (hx, hy) = hero_point();
            game.pointer(hx, hy, 2);
            let mut waited = 0;
            while drawn(&game).is_none() && waited < 8 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
                waited += 1;
                // Round 13 (opus N1): the slide still runs on every frame before the menu's first.
                assert!(
                    drawn(&game).is_some() || game.sliding(),
                    "{label}: the slide running before the menu's first frame ({waited} frames)"
                );
            }
            let v = drawn(&game).expect("the menu open");
            let opened = game.presentation_ms();
            let after = game.motion.presenter.generation();
            let none =
                |game: &Game| game.slide().is_empty() && !game.motion.drawn && !game.sliding();
            assert!(
                after != before,
                "{label}: the slide ended at once (the presenter's generation moved)"
            );
            assert!(
                none(&game),
                "{label}: no slide drawn or placed under the menu"
            );
            assert_eq!(game.history_offset() as usize, back, "{label}: the history");
            {
                let e = game.engine.as_ref().quiet();
                let (screen, viewed) = (e.screen_frame(), viewed_frame(e));
                let (w, h) = (u6_render::WIDTH, u6_render::HEIGHT);
                assert_eq!(
                    screen.pixels().len(),
                    w * h,
                    "{label}: the screen frame's size"
                );
                assert_eq!(viewed.pixels().len(), w * h, "{label}: the expected size");
                let (x0, y0, x1, y1) = v.rect;
                let boxed = |x: usize, y: usize| {
                    (x0..=x1).contains(&(x as i32)) && (y0..=y1).contains(&(y as i32))
                };
                // The expected shown frame: the box's pixels the screen frame's, the sprite over.
                let mut want = viewed.clone();
                let (mut inside, mut differ, mut outside) = (0, 0, 0);
                for y in 0..h {
                    for x in 0..w {
                        let (a, b) = (screen.get(x, y), viewed.get(x, y));
                        if boxed(x, y) {
                            inside += 1;
                            differ += usize::from(a != b);
                            want.set(x, y, a.quiet());
                        } else {
                            outside += usize::from(a != b);
                        }
                    }
                }
                assert_eq!(
                    outside, 0,
                    "{label}: the screen frame outside the menu's box: the published frame and the view"
                );
                assert!(
                    differ * 2 > inside,
                    "{label}: the menu drawn in its box: {differ} of {inside}"
                );
                let history = (0..w * h)
                    .filter(|&i| viewed.pixels()[i] != e.published_frame().pixels()[i])
                    .count();
                assert_eq!(history > 0, scrolled, "{label}: the history's view drawn");
                let screen_want = want.clone();
                if let Some(sprite) = e.sprite() {
                    sprite.draw(&mut want);
                }
                assert_eq!(e.frame().pixels().len(), w * h, "{label}: the shown size");
                assert!(
                    e.frame().pixels() == want.pixels(),
                    "{label}: the shown frame: the screen frame with the sprite over it"
                );
                if hd {
                    let want = hd_words_of(e, &screen_want);
                    let shown = words_of(exported(game.hd_frame_ptr(), game.hd_frame_len()));
                    assert_eq!(shown.len(), want.len(), "{label}: the HD frame's size");
                    assert!(
                        shown == want,
                        "{label}: the page shows the HD frame of the screen frame"
                    );
                } else {
                    let mut rgba = Vec::new();
                    want.write_rgba(&mut rgba);
                    let shown = game.frame_rgba();
                    assert_eq!(shown.len(), rgba.len(), "{label}: the RGBA frame's size");
                    assert!(
                        shown == rgba,
                        "{label}: the RGBA frame is the shown frame, the menu in it"
                    );
                }
            }
            // The menu's press let go (the menu's), Escape closing it a few frames on; through the
            // slide's own end and past it no slide comes back.
            game.pointer(hx, hy, 0);
            let (mut open, mut closed) = (0, 0);
            while game.presentation_ms() < opened + u6_render::motion::SLIDE_MS + 200.0 {
                if open == 6 && drawn(&game).is_some() {
                    assert!(down(&mut game, "Escape"));
                    assert!(game.key_up("Escape"));
                }
                page_frame_at(&mut game, &mut page, &mut |_| {});
                if drawn(&game).is_some() {
                    open += 1;
                } else {
                    closed += 1;
                }
                assert!(
                    none(&game) && game.motion.presenter.generation() == after,
                    "{label}: no slide resumes (menu open {open} frames, closed {closed})"
                );
            }
            assert!(
                open >= 6 && closed > 0,
                "{label}: frames with the menu open ({open}) and after it closed ({closed})"
            );
            assert_eq!(
                game.history_offset() as usize,
                back,
                "{label}: the history after"
            );
            // Round 13 (opus S3): motion resumes after the menu. Out of the fight a step tapped
            // slides as a held walk's paced step must, and ends at rest (Go here, refused here
            // with a foe in view, is the castle's: `go_here_chosen_from_a_menu_slides_…`).
            let mut resumed = String::new();
            if !scrolled {
                let p = tap_step(&mut game, &mut page).expect("a step after the menu");
                assert!(
                    slid_at_1x(&p),
                    "{label}: the step after the menu slides: {p:?}"
                );
                resumed.push_str(&format!(
                    "; the step after it: {} moving frames, jump {} px",
                    p.moving, p.jump
                ));
            }
            println!(
                "{label}: a right press during a slide ({taps} keys tapped to one): the slide \
                 ended at the menu's first frame, each layer where the composition puts it; \
                 {open} frames with the menu open and {closed} after it closed, none sliding\
                 {resumed}"
            );
        }
    }

    /// The held walk's predicate on one step at 1× ([`a_held_walk_slides_on_every_paced_step_and_its_moving_share_by_speed`]):
    /// [`moving_needed`] moving frames or more, none backward or stray, no jump over
    /// [`jump_bound`], and at rest by its close.
    fn slid_at_1x(p: &Progress) -> bool {
        p.moving >= moving_needed(1.0)
            && p.backward == 0
            && p.stray == 0
            && p.jump <= jump_bound(1.0)
            && !p.unfinished
    }

    /// Spec 20 with spec 17 and 21 (round 13, opus S3, the second variant): after a context menu,
    /// Go here chosen starts the travel and its first step slides (a presenter left stuck by the
    /// menu fails). In the castle ([`in_the_castle`]: no foe in view, so Go here can run; the
    /// start room's fight leaves foes in view, which refuse it) at 1×, HD off and on, after two
    /// passes: a right press on a cell two from the hero whose Go here can run; the menu's frames
    /// show no slide; its press let go, Go here chosen, the travel's first step slides as a held
    /// walk's paced step must ([`slid_at_1x`], counted to its second step or to rest). The menu
    /// opens at rest here: no NPC of the castle slides at a prompt (none in 64 keys tapped, nor
    /// one set to walk at random within five cells), so the menu's snap of a running slide and the
    /// step after it are [`a_context_menu_opened_during_a_slide_ends_it_and_it_does_not_resume`]'s.
    #[test]
    fn go_here_chosen_from_a_menu_slides_the_travels_first_step() {
        for hd in [false, true] {
            let label = if hd { "HD" } else { "classic" };
            let Some(mut game) = in_the_castle() else {
                return;
            };
            game.engine.as_mut().quiet().set_context_menu(true);
            game.set_motion(true);
            if hd {
                game.set_hd(true, 2);
            }
            let mut page = Page::new();
            for _ in 0..2 {
                assert!(down(&mut game, "Space"));
                assert!(game.key_up("Space"));
                game.step(20);
                assert!(
                    steps_to_prompt(&mut game),
                    "{label}: the prompt after a pass"
                );
                game.present();
            }
            for _ in 0..60 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
            }
            let (hx, hy) = hero_point();
            let cell = [(32, 0), (-32, 0), (0, 32), (0, -32), (32, 32), (-32, -32)]
                .into_iter()
                .map(|(dx, dy)| (hx + dx, hy + dy))
                .find(|&(x, y)| {
                    let e = game.engine.as_ref().quiet();
                    u6_core::ui::menu::query(&e.state(), e.io(), x as i16, y as i16)
                        .and_then(|m| m.go_here)
                        .is_some_and(|g| g.enabled())
                })
                .expect("a cell two from the hero whose Go here can run");
            game.pointer(cell.0, cell.1, 2);
            for _ in 0..8 {
                if drawn(&game).is_some() {
                    break;
                }
                page_frame_at(&mut game, &mut page, &mut |_| {});
            }
            let v = drawn(&game).expect("the menu open on the cell");
            assert!(
                game.slide().is_empty() && !game.motion.drawn && !game.sliding(),
                "{label}: no slide under the menu"
            );
            game.pointer(cell.0, cell.1, 0);
            for _ in 0..4 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
            }
            let go = row_of(&v, u6_core::ui::menu::GO_HERE).expect("Go here");
            let (gx, gy) = row_point(&v, go);
            let at = game.leader_position();
            game.pointer(gx, gy, 1);
            // The travel's first step (it may come before the press is let go), counted to its
            // second (or to rest at the travel's end).
            let (mut first, mut still) = (None::<(Progress, Vec<u16>)>, 0);
            for f in 0..1800 {
                if f == 4 {
                    game.pointer(gx, gy, 0);
                    assert_eq!(game.go_here_asked(), 1, "{label}: Go here chosen");
                }
                let mut moved = None;
                page_frame_at(&mut game, &mut page, &mut |g| {
                    let now = g.leader_position();
                    let from = first.as_ref().map_or(&at, |f| &f.1);
                    if moved.is_none() && now != *from {
                        moved = Some(now);
                    }
                });
                match (moved, first.as_mut()) {
                    (Some(now), None) => {
                        let d = |i: usize| i32::from(now[i]) - i32::from(at[i]);
                        first = Some((Progress::new((d(0), d(1))), now));
                    }
                    (Some(_), Some(_)) => break,
                    _ => {}
                }
                if let Some((p, _)) = first.as_mut() {
                    p.frame(shown_offset(&game), game.old_cell(), true);
                }
                still = if !game.travel_active() && game.at_prompt() && !game.sliding() {
                    still + 1
                } else {
                    0
                };
                if first.is_some() && still >= 30 {
                    break;
                }
            }
            let (mut p, _) = first.expect("the travel's first step");
            p.close();
            println!(
                "{label}: Go here from the menu: the travel's first step {} moving frames, \
                 backward {}, stray {}, jump {} px, unfinished {}",
                p.moving, p.backward, p.stray, p.jump, p.unfinished
            );
            assert!(
                slid_at_1x(&p),
                "{label}: the travel's first step slides: {p:?}"
            );
        }
    }

    /// The page's game in its start room (skipped without the user's zip, failing with
    /// `U6_REQUIRE_ARCHIVE=1`): `game` at once with the seed the page's clock gives at the browser
    /// receipts' fixed time (`clockSeed` in site/app.js: 1 700 000 000 s, its low word), smooth
    /// motion on, HD on at 2× with `hd`, the start fight broken off with `b` (the assault's
    /// actors not passed over, as in the page), at the prompt.
    fn in_the_start_room(hd: bool) -> Option<Game> {
        in_the_start_room_seeded(hd, (1_700_000_000u64 & 0xffff) as u16)
    }

    /// [`in_the_start_room`] with the game's seed `seed`.
    #[allow(clippy::disallowed_methods)] // tests may read files and the environment
    fn in_the_start_room_seeded(hd: bool, seed: u16) -> Option<Game> {
        if !u6_testkit::archive().exists() {
            let required = std::env::var("U6_REQUIRE_ARCHIVE").is_ok_and(|v| v == "1");
            assert!(
                !required,
                "U6_REQUIRE_ARCHIVE=1 and the local archive not present"
            );
            eprintln!("start room: skipped (the local archive not present)");
            return None;
        }
        eprintln!("start room: ran");
        let mut game = Game::new();
        game.collect(
            "ultima-vi.zip",
            std::fs::read(u6_testkit::archive()).quiet(),
        )
        .quiet();
        game.set_motion(true);
        if hd {
            game.set_hd(true, 2);
        }
        game.boot(seed, true).quiet();
        assert!(steps_to_prompt(&mut game), "the prompt");
        let t = game.clock;
        assert!(game.key_down("KeyB", "b", false, t));
        assert!(game.key_up("KeyB"));
        game.step(20);
        assert!(steps_to_prompt(&mut game), "the prompt out of the fight");
        game.present();
        Some(game)
    }

    /// [`in_the_start_room`] in its start fight (not broken off: the party in combat mode, each
    /// member's turn a prompt), at the first prompt.
    #[allow(clippy::disallowed_methods)] // tests may read files and the environment
    fn in_the_start_fight(hd: bool) -> Option<Game> {
        if !u6_testkit::archive().exists() {
            let required = std::env::var("U6_REQUIRE_ARCHIVE").is_ok_and(|v| v == "1");
            assert!(
                !required,
                "U6_REQUIRE_ARCHIVE=1 and the local archive not present"
            );
            eprintln!("start fight: skipped (the local archive not present)");
            return None;
        }
        eprintln!("start fight: ran");
        let mut game = Game::new();
        game.collect(
            "ultima-vi.zip",
            std::fs::read(u6_testkit::archive()).quiet(),
        )
        .quiet();
        game.set_motion(true);
        if hd {
            game.set_hd(true, 2);
        }
        game.boot((1_700_000_000u64 & 0xffff) as u16, true).quiet();
        assert!(steps_to_prompt(&mut game), "the prompt");
        game.present();
        Some(game)
    }

    /// Spec 20 Design 3–4 (rounds 11–13, opus S3, S2, gpt a S1, S3; round 13 MUST 2 and 3), on
    /// the user's zip, in the page's start room ([`in_the_start_room`]), where a held walk's turns
    /// take seconds: at 1× on 60 Hz frames, with HD off and on, each way held in turn until the
    /// party stops for 10 s (a turn of the Wide view, with more actors in it, can take longer than
    /// 5 s), then 300 steps with no key (the last turn drained). Per early picture (each step's,
    /// [`Engine::take_motion`]'s previews), the engine ms to the first publication after it that
    /// is not a preview, which every early picture has; per step that has one (round 13: a step
    /// whose turn's first pick fights has none, spec 20 Design 4), the frames between the step and
    /// the turn's picture (the first scene of the engine's after it, round 13: a foreign
    /// publication is not it, opus S1) that showed the engine's frame (the party back on its old
    /// cell: with HD off no slide drawn in the RGBA frame, with HD on no slide placed), counted
    /// apart when a foreign picture came after the early picture with no scene since
    /// ([`Game::old_cell`]: the old cell through a foreign picture). No such frame is shown. The
    /// premises: some turn takes longer than the longest wait to the turn's first publication (so
    /// the hold's end at that publication, not at the turn's end or its prompt, is what is
    /// measured), and the game's key waits refresh the view (A4-0x7a56, spec 20 Design 3). And
    /// the proxy (gpt a S3): on every frame that draws the presenter's picture, its newest scene
    /// is centred on the leader's cell now, so a stale scene drawn over the old cell counts.
    #[test]
    fn the_early_picture_is_held_until_the_turns_picture_in_the_start_room() {
        for hd in [false, true] {
            let label = if hd { "HD" } else { "classic" };
            let Some(mut game) = in_the_start_room(hd) else {
                return;
            };
            assert!(
                game.engine.as_ref().quiet().state().ui.input.idle_refresh,
                "{label}: the key waits refresh the view (A4-0x7a56)"
            );
            let mut page = Page::new();
            let now = |g: &Game| g.engine.as_ref().map_or(0.0, |e| e.io().now_ms() as f64);
            // Per step: its engine ms, the frames after it that showed the engine's frame before
            // the turn's picture, and those of them through a foreign picture.
            let mut steps: Vec<(f64, usize, usize)> = Vec::new();
            // Frames drawing the presenter's picture, and those whose newest scene was not
            // centred on the leader's cell.
            let (mut drawing, mut stale) = (0, 0);
            // The index of each hold's first step, and past the last.
            let mut holds = Vec::new();
            for key in ["ArrowDown", "ArrowRight", "ArrowLeft", "ArrowUp"] {
                holds.push(steps.len());
                assert!(down(&mut game, key));
                let (mut at, mut still) = (game.leader_position(), 0);
                for _ in 0..7200 {
                    if still >= 600 {
                        break;
                    }
                    page_frame_at(&mut game, &mut page, &mut |g| {
                        if g.leader_position() != at {
                            at = g.leader_position();
                            steps.push((now(g), 0, 0));
                            still = 0;
                        }
                    });
                    still += 1;
                    let Some(step) = steps.last_mut() else {
                        continue;
                    };
                    let taken = &game.motion.taken;
                    let pictured =
                        (taken.iter()).any(|&(t, preview, shot)| !preview && shot && t >= step.0);
                    let early = (taken.iter()).any(|&(t, preview, _)| preview && t >= step.0);
                    let presenter = if hd {
                        !game.slide().is_empty()
                    } else {
                        game.motion.drawn
                    };
                    if early && !pictured && !presenter {
                        if game.old_cell() {
                            step.2 += 1;
                        } else {
                            step.1 += 1;
                        }
                    }
                    if presenter {
                        let newest = game.motion.presenter.history().first();
                        let c = newest.map(|n| n.scene.centre);
                        let p = game.leader_position();
                        let leader = (i32::from(p[0]), i32::from(p[1]), p[2] as u8);
                        drawing += 1;
                        stale += usize::from(c != Some(leader));
                    }
                }
                assert!(game.key_up(key));
                for _ in 0..300 {
                    step_1x(&mut game);
                    game.present();
                }
            }
            holds.push(steps.len());
            let taken = &game.motion.taken;
            // Per preview: the ms to the next publication that is not one; none after the drain
            // is an early picture left unresolved, which fails.
            let matched: Vec<Option<f64>> = (taken.iter().enumerate())
                .filter(|(_, p)| p.1)
                .map(|(i, &(t, _, _))| taken[i + 1..].iter().find(|p| !p.1).map(|n| n.0 - t))
                .collect();
            let unmatched = matched.iter().filter(|w| w.is_none()).count();
            assert_eq!(
                unmatched,
                0,
                "{label}: early pictures with no publication after them, the last turn drained: \
                 {unmatched} of {}",
                matched.len()
            );
            let waits: Vec<f64> = matched.into_iter().flatten().collect();
            let back: Vec<usize> = steps.iter().map(|s| s.1).collect();
            let through: usize = steps.iter().map(|s| s.2).sum();
            let longest = waits.iter().copied().fold(0.0, f64::max);
            // The engine ms between two steps of the same hold (the turns).
            let turns: Vec<f64> = (holds.windows(2))
                .flat_map(|h| (h[0]..h[1].saturating_sub(1)).map(|i| steps[i + 1].0 - steps[i].0))
                .collect();
            println!(
                "{label}, the start room at 1x: {} steps ({} with no early picture), engine ms \
                 between them {turns:?}; {} early pictures, engine ms from each to the turn's \
                 first publication {waits:?}, the longest {longest}; per step the frames showing \
                 the engine's frame before the turn's picture {back:?}; frames of the old cell \
                 through a foreign picture {through}",
                steps.len(),
                steps.len().saturating_sub(waits.len()),
                waits.len()
            );
            assert!(
                waits.len() >= 4,
                "{label}: early pictures in the start room"
            );
            assert!(
                turns.iter().any(|&t| t > longest),
                "{label}: the premise, a turn longer than the wait to its first publication: \
                 {turns:?}"
            );
            assert!(
                back.iter().all(|&n| n == 0),
                "{label}: the engine's frame (the old cell) shown before the turn's picture: {back:?}"
            );
            assert_eq!(
                through, 0,
                "{label}: the old cell through a foreign picture after an early picture"
            );
            assert!(
                drawing > 0 && stale == 0,
                "{label}: the presenter's pictures on the leader's cell: {stale} of {drawing} not"
            );
            println!(
                "{label}: {drawing} frames drawing the presenter's picture, each centred on the \
                 leader's cell; every early picture matched to a later publication"
            );
        }
    }

    /// Spec 20 Design 3 (round 13, MUST 2: gpt a and opus MUST): the early picture is held until
    /// the turn's first publication, however long the turn's first pick waits. In the castle
    /// ([`castle_with_state`]) with no NPC that acts in the window (every actor outside the party
    /// passed over, as the assault's) and the leader's step leaving it 128 points in debt, forced
    /// in the tick of the step (the real data's vessel steps, the costs aboard a ship or a raft
    /// against the avatar's dexterity, leave at most one refill: the most a debt of 128 points
    /// leaves is nine, `turn.rs` `refill` carrying the debt), so the scheduler refills nine times
    /// (a pass and a clock minute each) before it picks the leader: the turn's first picture comes
    /// more than 2000 ms (round 12's `PREVIEW_MS`) after the early picture. At 1× on 60 Hz frames,
    /// HD off and on, the down arrow tapped after two passes: on every frame from the step to
    /// that publication the presenter's picture shows (with HD off a slide drawn in the RGBA
    /// frame, with HD on one placed), centred on the leader's cell; the publication is a scene of
    /// the engine's. On round 12's presenter the engine's frame (the party on its old cell) showed
    /// from 2000 ms to the refresh. With HD off a frame 2200 ms after the step is written for the
    /// dispatcher (`hold-long-first-pick-classic.png`, [`keep_frame`]).
    #[test]
    fn an_early_picture_is_held_through_a_long_first_pick_until_the_turns_picture() {
        for hd in [false, true] {
            let label = if hd { "HD" } else { "classic" };
            let Some((mut game, st)) = castle_with_state() else {
                return;
            };
            {
                let mut s = st.borrow_mut();
                for a in 0..0x100 {
                    if s.actors.attrs.status[a] & 0x80 == 0 {
                        s.actors.attrs.status[a] |= 0x10;
                    }
                }
            }
            game.set_motion(true);
            if hd {
                game.set_hd(true, 2);
            }
            let mut page = Page::new();
            for _ in 0..2 {
                assert!(down(&mut game, "Space"));
                assert!(game.key_up("Space"));
                game.step(20);
                assert!(
                    steps_to_prompt(&mut game),
                    "{label}: the prompt after a pass"
                );
                game.present();
            }
            for _ in 0..120 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
            }
            assert!(
                !game.motion.presenter.history().is_empty(),
                "{label}: a scene to slide from"
            );
            let now = |g: &Game| g.engine.as_ref().map_or(0.0, |e| e.io().now_ms() as f64);
            let leader = usize::from(st.borrow().actors.party.leader().quiet().0);
            let at = game.leader_position();
            assert!(down(&mut game, "ArrowDown"));
            assert!(game.key_up("ArrowDown"));
            let mut step: Option<f64> = None;
            let (mut before, mut back, mut drawn, mut stale, mut kept) = (0, 0, 0, 0, false);
            let mut first = None;
            for _ in 0..1800 {
                page_frame_at(&mut game, &mut page, &mut |g| {
                    if step.is_none() && g.leader_position() != at {
                        // The step's debt: the most a point count holds below 0.
                        st.borrow_mut().actors.movement_points[leader] = 0x80;
                        step = Some(now(g));
                    }
                });
                let Some(t0) = step else {
                    continue;
                };
                if first.is_none() {
                    first = (game.motion.taken.iter())
                        .find(|p| !p.1 && p.0 >= t0)
                        .copied();
                }
                if first.is_some() {
                    if game.at_prompt() {
                        break;
                    }
                    continue;
                }
                before += 1;
                let presenter = if hd {
                    !game.slide().is_empty()
                } else {
                    game.motion.drawn
                };
                back += usize::from(!presenter);
                if presenter {
                    let c = game
                        .motion
                        .presenter
                        .history()
                        .first()
                        .map(|n| n.scene.centre);
                    let p = game.leader_position();
                    drawn += 1;
                    stale += usize::from(c != Some((i32::from(p[0]), i32::from(p[1]), p[2] as u8)));
                }
                if !hd && !kept && now(&game) - t0 >= 2200.0 {
                    let rgba = game.frame_rgba();
                    kept = keep_frame(
                        "hold-long-first-pick-classic.png",
                        (u6_render::WIDTH, u6_render::HEIGHT),
                        &rgba,
                    );
                }
            }
            let t0 = step.expect("the step");
            let early = (game.motion.taken.iter())
                .filter(|p| p.1 && p.0 >= t0)
                .count();
            let (t1, _, scene) = first.expect("the turn's first publication");
            println!(
                "{label}: a step in debt, no NPC acting: its early picture's hold {} engine ms to \
                 the turn's first publication; {before} frames between, {drawn} drawing the \
                 presenter's picture, {back} the engine's frame (the old cell), {stale} stale; \
                 a frame {}",
                t1 - t0,
                if kept { "written" } else { "not written" }
            );
            assert_eq!(early, 1, "{label}: the step's early picture");
            assert!(
                t1 - t0 > 2000.0 && scene,
                "{label}: the premise, the turn's first publication a scene past 2000 ms: {} ms",
                t1 - t0
            );
            assert_eq!(
                back, 0,
                "{label}: the engine's frame (the old cell) shown before the turn's picture"
            );
            assert!(
                drawn == before && stale == 0,
                "{label}: the presenter's picture on the leader's cell: {drawn} of {before}, {stale} stale"
            );
        }
    }

    /// Spec 20 Design 3 (round 13): an early picture whose turn publishes nothing more of the map
    /// ends at the command prompt ([`Presenter::prompt`], from [`Game::slide_view`]). In the
    /// castle at 1×, HD off, the down arrow tapped after two passes; once the presenter took the
    /// step's early picture, every publication the engine queues is dropped before the page
    /// presents (a turn whose picture did not change the shown map). Until the prompt is back the
    /// presenter's picture shows (the engine's frame holds the old cell); at the first present
    /// with the command prompt up nothing is held or drawn. The castle's turn ends well inside
    /// round 12's 2000 ms timer, which held it past the prompt.
    #[test]
    fn an_early_picture_with_nothing_published_after_it_ends_at_the_prompt() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        game.set_motion(true);
        let mut page = Page::new();
        for _ in 0..2 {
            assert!(down(&mut game, "Space"));
            assert!(game.key_up("Space"));
            game.step(20);
            assert!(steps_to_prompt(&mut game), "the prompt after a pass");
            game.present();
        }
        for _ in 0..120 {
            page_frame_at(&mut game, &mut page, &mut |_| {});
        }
        let now = |g: &Game| g.engine.as_ref().map_or(0.0, |e| e.io().now_ms() as f64);
        let at = game.leader_position();
        let takes = game.engine.as_ref().quiet().prompt_takes();
        assert!(down(&mut game, "ArrowDown"));
        assert!(game.key_up("ArrowDown"));
        let (mut step, mut prompt) = (None, None);
        let (mut frames, mut held) = (0, 0);
        for _ in 0..1800 {
            page_frame_at(&mut game, &mut page, &mut |g| {
                if g.motion.taken.iter().any(|p| p.1) {
                    drop(g.engine.as_mut().quiet().take_motion());
                }
            });
            if step.is_none() && game.leader_position() != at {
                step = Some(now(&game));
            }
            let Some(t0) = step else {
                continue;
            };
            let e = game.engine.as_ref().quiet();
            if e.prompt_takes() > takes && u6_core::engine::command_prompt(&e.state()) {
                prompt = Some((now(&game) - t0, game.sliding(), game.motion.drawn));
                break;
            }
            frames += 1;
            held += usize::from(game.motion.drawn);
        }
        let (ms, sliding, drawn) = prompt.expect("the prompt back");
        let early = game.motion.taken.iter().filter(|p| p.1).count();
        println!(
            "an early picture with nothing published after it: {early} taken; the prompt back \
             {ms} engine ms after the step; {held} of the {frames} frames before it drawing the \
             presenter's picture; at the prompt held {sliding}, drawn {drawn}"
        );
        assert_eq!(early, 1, "the step's early picture taken");
        assert!(
            ms < 2000.0,
            "the premise: the prompt inside round 12's timer"
        );
        assert!(
            frames > 0 && held == frames,
            "the presenter's picture until the prompt"
        );
        assert!(!sliding && !drawn, "the hold ended at the prompt");
    }

    /// Spec 20 Design 4 (round 13, MUST 3: opus S1), on the user's zip: out of combat mode an NPC
    /// in a combat routine that the scheduler picks first after the leader's step plays its
    /// frames before the turn's refresh over the window as last published (a flight's steps), so
    /// an early picture would show the party slide, then back on its old cell, then on the new
    /// one. In the page's start room (HD off, 1×), seed 25402 in Original and 7 in Wide (each
    /// exercises a first-fighter publication), where the start fight's actors,
    /// left out of combat mode by B, shoot: a step toward the nearest of them, or beside it once
    /// adjacent (down, right, up, left, a pass, by turns), 40 times. Per step whether a foreign
    /// publication came before the turn's first scene (the fight's frames first), whether the
    /// step had an early picture, and the frames that showed the engine's frame through a foreign
    /// picture after the early picture with no scene since ([`Game::old_cell`]). Some step's
    /// first pick shows a fight's frames (the premise), none of those steps has an early picture,
    /// and no frame shows the old cell through a foreign picture. At the first such step the frame
    /// before its first foreign publication and the one with it are written for the dispatcher
    /// (`fight-first-pick-before.png`, `fight-first-pick-at.png`, [`keep_frame`]).
    #[test]
    fn a_fighter_first_after_the_step_gets_no_early_picture_and_no_old_cell_shows() {
        let seed = if u6_render::WIDTH > 320 { 7 } else { 25402 };
        let Some(mut game) = in_the_start_room_seeded(false, seed) else {
            return;
        };
        let fighter = |wt: u8| matches!(wt, 3..=11 | 13 | 14 | 0x13 | 0x9a | 0x9b);
        // The nearest actor out of the party in a combat routine, as (dx, dy, Chebyshev distance).
        let nearest = |g: &Game| {
            let e = g.engine.as_ref().quiet();
            let s = e.state();
            let c = s.map.centre;
            let mut best: Option<(i32, i32, i32)> = None;
            for a in 0..0x100usize {
                let (st, p) = (s.actors.attrs.status[a], s.objects.location[a]);
                if s.objects.type_frame[a] == 0
                    || st & 0x90 != 0
                    || !fighter(s.actors.worktype[a])
                    || p.z != c.z
                {
                    continue;
                }
                let (dx, dy) = (
                    i32::from(p.x) - i32::from(c.x),
                    i32::from(p.y) - i32::from(c.y),
                );
                let d = dx.abs().max(dy.abs());
                if best.is_none_or(|b| d < b.2) {
                    best = Some((dx, dy, d));
                }
            }
            best
        };
        let mut page = Page::new();
        let (mut steps, mut tries) = (0, 0);
        let (mut fights, mut fights_early, mut through, mut kept) = (0, 0, 0, false);
        while steps < 40 && tries < 300 {
            tries += 1;
            let Some((dx, dy, d)) = nearest(&game) else {
                break;
            };
            let key = match () {
                _ if d <= 1 => {
                    ["ArrowDown", "ArrowRight", "ArrowUp", "ArrowLeft", "Space"][tries % 5]
                }
                _ if dx.abs() >= dy.abs() => ["ArrowLeft", "ArrowRight"][usize::from(dx > 0)],
                _ => ["ArrowUp", "ArrowDown"][usize::from(dy > 0)],
            };
            let at = game.leader_position();
            let takes = game.engine.as_ref().quiet().prompt_takes();
            let n0 = game.motion.taken.len();
            let mut frame_before = game.frame_rgba();
            assert!(down(&mut game, key));
            assert!(game.key_up(key));
            let (mut moved, mut first_foreign) = (false, false);
            for _ in 0..1200 {
                page_frame_at(&mut game, &mut page, &mut |_| {});
                moved |= game.leader_position() != at;
                let t = &game.motion.taken[n0..];
                let scene = t.iter().position(|p| !p.1 && p.2).unwrap_or(t.len());
                let foreign = t[..scene].iter().any(|p| !p.2);
                if moved && foreign && !first_foreign {
                    first_foreign = true;
                    if !kept {
                        let size = (u6_render::WIDTH, u6_render::HEIGHT);
                        kept = keep_frame("fight-first-pick-before.png", size, &frame_before)
                            & keep_frame("fight-first-pick-at.png", size, &game.frame_rgba());
                    }
                }
                through += usize::from(moved && game.old_cell() && !game.motion.drawn);
                frame_before = game.frame_rgba();
                let e = game.engine.as_ref().quiet();
                if e.prompt_takes() > takes && game.at_prompt() && game.pending.is_empty() {
                    break;
                }
            }
            if moved {
                steps += 1;
                let t = &game.motion.taken[n0..];
                if first_foreign {
                    fights += 1;
                    fights_early += usize::from(t.iter().any(|p| p.1));
                }
            }
        }
        println!(
            "seed {seed}, the start room out of its fight: {steps} steps in {tries} keys; {fights} \
             whose turn showed a fight's frames before its first scene, {fights_early} of them with \
             an early picture; frames of the old cell through a foreign picture {through}; the \
             frame pair {}",
            if kept { "written" } else { "not written" }
        );
        assert!(fights > 0, "the premise: a fighter first after a step");
        assert_eq!(
            fights_early, 0,
            "no early picture when the first pick fights"
        );
        assert_eq!(through, 0, "the old cell through a foreign picture");
    }

    /// The published frame of `e` with the message history's view over it, composed here apart
    /// from the engine's composition (spec 23 Decision 12: what its screen frame is with no menu
    /// open), from the published frame and the view alone.
    fn viewed_frame(e: &Engine) -> Framebuffer {
        let live = e.published_frame();
        let mut fb = live.clone();
        let view = e.history_view();
        if view.offset() > 0 {
            u6_core::ui::scrollback::draw_view(&e.state().ui, view, &live, &mut fb);
        }
        fb
    }

    /// The offset from its rest place of the static layer the page draws now ([`Game::slide`]'s
    /// triple of the layer whose role is the static words, [`Game::slide_layer_role`]), its rest
    /// place a cell up and left of the frame layer's (the cursor and the border, drawn at rest,
    /// found by its role too); `None` with no slide placed.
    fn static_offset(game: &Game) -> Option<(i32, i32)> {
        let placed = game.slide();
        let place = |role: i32| {
            (placed.chunks_exact(3))
                .find(|t| game.slide_layer_role(t[0] as u32) == role)
                .map(|t| (t[1], t[2]))
        };
        let (sx, sy) = place(0)?;
        let i = placed
            .chunks_exact(3)
            .find(|t| game.slide_layer_role(t[0] as u32) == 0)?[0];
        let origin = game.slide_layer_origin(i as u32);
        Some((sx - origin[0], sy - origin[1]))
    }

    /// Spec 19 O1c (round 4 item 1), the owner's symptom on a real keyboard above 4×: the down
    /// arrow held 1 s with the browser's repeat (the first 225 wall ms after the press, then one
    /// every 30 ms) at 6×, 8× and Max, where every castle turn is shorter than the minimum
    /// interval. The command prompt takes the repeats back, so the paced press is its one step
    /// source and every gap between two steps is 150 wall ms within one frame; a repeat waiting
    /// at the prompt would step at once (a turn's 80 to 143 ms at 6× and 8×, about a step a
    /// repeat at Max). Written on the API round 3 had, so it runs on that code as it is.
    #[test]
    fn above_4x_a_held_walk_with_the_browsers_repeat_keeps_the_minimum_interval() {
        for speed in [6.0, 8.0, 0.0] {
            let Some(mut game) = in_the_castle() else {
                return;
            };
            game.set_speed(speed);
            let t0 = game.clock;
            assert!(game.key_down("ArrowDown", "ArrowDown", false, t0));
            let mut at = game.leader_position();
            let mut steps = Vec::new();
            let mut next_repeat = 225.0;
            while game.clock - t0 < 1000.0 {
                while game.clock - t0 + FRAME > next_repeat {
                    assert!(game.key_repeat("ArrowDown", "ArrowDown", false, t0 + next_repeat));
                    next_repeat += 30.0;
                }
                frame(&mut game, FRAME, &mut |g| {
                    if g.leader_position() != at {
                        at = g.leader_position();
                        steps.push(g.clock - t0);
                    }
                });
            }
            assert!(game.key_up("ArrowDown"));
            let gaps: Vec<f64> = steps.windows(2).map(|w| w[1] - w[0]).collect();
            let shown: Vec<f64> = gaps.iter().map(|g| g.round()).collect();
            println!(
                "held 1 s at speed {speed} with the browser's repeat: {} steps, gaps {shown:?} wall ms",
                steps.len()
            );
            assert!(gaps.len() >= 4, "steps while held at speed {speed}");
            assert!(
                gaps.iter().all(|&g| (g - MIN).abs() <= FRAME + 0.01),
                "150 ms within a frame at speed {speed} (0: Max): {shown:?}"
            );
        }
    }

    /// Spec 19 O1c (round 4 item 3): the minimum interval holds between steps however late a
    /// frame took the press. At Max the arrow is pressed at the page's clock t0 and the first
    /// frame comes 200 ms late, so the press is taken there, 200 ms after its event; then 60 Hz
    /// frames. The second step comes 150 ms after the first (the interval runs from the step the
    /// prompt took), not within that frame because the press is already 150 ms old.
    #[test]
    fn a_late_first_frame_keeps_the_minimum_interval_after_the_first_step() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        game.set_speed(0.0);
        let t0 = game.clock;
        assert!(down(&mut game, "ArrowDown"));
        let mut at = game.leader_position();
        let mut steps = Vec::new();
        let mut seen = |g: &Game| {
            if g.leader_position() != at {
                at = g.leader_position();
                steps.push(g.clock - t0);
            }
        };
        frame(&mut game, 200.0, &mut seen);
        while game.clock - t0 < 700.0 {
            frame(&mut game, FRAME, &mut seen);
        }
        assert!(game.key_up("ArrowDown"));
        let shown: Vec<f64> = steps.iter().map(|t| t.round()).collect();
        println!("a 200 ms first frame at Max: steps at {shown:?} wall ms from the press");
        assert!(steps.len() >= 3, "a walk: {shown:?}");
        assert_eq!(steps[0], 200.0, "the press, in the late frame");
        let gaps: Vec<f64> = steps.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            gaps.iter().all(|&g| g >= MIN - 0.01),
            "150 ms between steps: {shown:?}"
        );
    }

    /// Spec 19 O1c (review, round 3 item 3; round 4 item 9): the first step comes at once, in
    /// the first step() after the press, and on the page's clock in the first frame that runs a
    /// step: at 8× and Max the first frame after the press; at 1×, where a step is 20 wall ms,
    /// the second 60 Hz frame (the pacer's, not the walk's).
    #[test]
    fn the_first_step_follows_the_first_step_call_after_the_press() {
        for speed in [1.0, 8.0, 0.0] {
            let Some(mut game) = in_the_castle() else {
                return;
            };
            let (cells, first) = press_for(&mut game, speed, 120.0);
            let ms = cells.first().map_or(f64::NAN, |c| c.0);
            println!("speed {speed}: the first step after {first} step() call(s), {ms:.1} wall ms");
            assert!(!cells.is_empty() && cells[0].1, "a step while down");
            assert_eq!(first, 1, "the first step() after the press, at {speed}x");
            let frames = if speed == 0.0 {
                1.0
            } else {
                (f64::from(pacer::STEP_MS) / speed / FRAME).ceil()
            };
            assert!(
                ms <= frames * FRAME + 0.01,
                "within {frames} frame(s) at {speed}x: {ms} ms"
            );
        }
    }

    /// Spec 19 O1c (review S3), the owner's symptom at 4× with the browser's repeat (the first
    /// 225 wall ms after the press, then every 30 ms), the castle's turns 160 to 215 wall ms
    /// there, longer than the minimum interval: per step of the leader, the steps it waited after
    /// the prompt came back. On the code before O1c the second waits out the OS delay (the first
    /// repeat comes after the prompt), the third not (repeats wait in the queue); with O1c the
    /// second waits at most one step. The turns' own lengths alternate (640 / 860 engine ms with
    /// the clock minute), so the waits after the prompt are compared, not the gaps between steps.
    #[test]
    fn at_4x_a_held_arrow_with_the_browsers_repeat_waits_no_os_delay() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        game.set_speed(4.0);
        let t0 = game.clock;
        assert!(down(&mut game, "ArrowDown"));
        let mut at = game.leader_position();
        let (mut read_at, mut i) = (Some(0usize), 0usize);
        let mut waits: Vec<Option<usize>> = Vec::new();
        let mut next_repeat = 225.0;
        while waits.len() < 3 && game.clock - t0 < 5000.0 {
            while game.clock - t0 + FRAME > next_repeat {
                let at_repeat = t0 + next_repeat;
                assert!(game.key_repeat("ArrowDown", "ArrowDown", false, at_repeat));
                next_repeat += 30.0;
            }
            frame(&mut game, FRAME, &mut |g| {
                i += 1;
                if g.leader_position() != at {
                    at = g.leader_position();
                    waits.push(read_at.take().map(|r| i - r));
                } else if read_at.is_none() && reading(g) {
                    read_at = Some(i);
                }
            });
        }
        assert!(game.key_up("ArrowDown"));
        println!("held down at 4x with repeats: steps after the prompt {waits:?}");
        assert_eq!(waits.len(), 3, "three steps");
        assert!(
            waits[1..].iter().all(|w| w.is_none_or(|n| n <= 1)),
            "no wait for the OS repeat after the prompt: {waits:?}"
        );
    }

    /// Spec 19 O1c (review round 2, MUST 2): a press is timed from its own event, not from the
    /// frame before it. A frame at 1000 ms on the page's clock, the arrow pressed at 1100 (its
    /// `timeStamp`), the next frame late, at 1240: down 140 ms there, not due; at 1250 it is.
    #[test]
    fn a_press_between_frames_is_timed_from_its_event() {
        let mut game = Game::new();
        game.engine = Some(reader(true, Rc::default()));
        game.set_clock(1000.0);
        game.step(20);
        assert!(game.key_down("ArrowLeft", "ArrowLeft", false, 1100.0));
        game.set_clock(1240.0);
        assert_eq!(game.held_due(), None, "140 ms down: not due");
        game.set_clock(1250.0);
        assert_eq!(game.held_due(), Some(0), "150 ms down: due");
    }

    /// Spec 19 O1c (round 4 item 12): a press whose event time is more than [`CLOCK_SKEW_MS`]
    /// from the page's clock (another timeline) is timed from the clock instead: pressed at
    /// 5000 with the clock at 1000, it is due at 1150, not at 5150; one 900 ms off keeps its own
    /// time.
    #[test]
    fn a_press_far_from_the_clock_is_timed_from_the_clock() {
        let mut game = Game::new();
        game.engine = Some(reader(true, Rc::default()));
        game.set_clock(1000.0);
        game.step(20);
        assert!(game.key_down("ArrowLeft", "ArrowLeft", false, 5000.0));
        assert_eq!(game.held[0].at, 1000.0, "the clock's time");
        game.set_clock(1150.0);
        assert_eq!(game.held_due(), Some(0), "due 150 ms after the clock");
        assert!(game.key_up("ArrowLeft"));
        assert!(game.key_down("ArrowLeft", "ArrowLeft", false, 2050.0));
        assert_eq!(game.held[0].at, 2050.0, "900 ms off: its own time");
    }

    /// Spec 19 O1c (review S1): a letter pressed while an arrow is held is read at the prompt
    /// it comes to, not dropped for the arrow's added press: held down until two steps, then `l`
    /// (Look) the moment the prompt is up; Look waits for its target (A4-0x7a60 set) and the
    /// leader did not step.
    #[test]
    fn a_letter_pressed_while_an_arrow_is_held_is_read() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        assert!(down(&mut game, "ArrowDown"));
        let mut at = game.leader_position();
        let mut cells = 0;
        for _ in 0..1500 {
            step_1x(&mut game);
            if game.leader_position() != at {
                at = game.leader_position();
                cells += 1;
            } else if cells >= 2 && reading(&game) {
                break;
            }
        }
        assert!(cells >= 2 && reading(&game), "two steps, the prompt up");
        let at_l = game.clock;
        assert!(game.key_down("KeyL", "l", false, at_l));
        step_1x(&mut game);
        let targeting = game.engine.as_ref().quiet().state().ui.input.targeting;
        assert_eq!(game.leader_position(), at, "no step for the arrow");
        assert_ne!(targeting, 0, "Look read, waiting for its target");
        game.key_up("KeyL");
        game.key_up("ArrowDown");
    }

    /// Spec 19 O1c (review S2): `release_keys` lets the held arrow go, so no press is added
    /// after it: the walk ends with the turn in flight; a new game put on screen holds no key.
    #[test]
    fn release_keys_ends_a_held_walk_and_a_new_game_holds_nothing() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        assert!(down(&mut game, "ArrowDown"));
        let mut at = game.leader_position();
        let mut cells = 0;
        for _ in 0..1500 {
            if cells == 2 {
                break;
            }
            step_1x(&mut game);
            if game.leader_position() != at {
                at = game.leader_position();
                cells += 1;
            }
        }
        assert_eq!(cells, 2, "two steps while held");
        game.release_keys();
        assert!(game.held.is_empty(), "nothing held");
        for _ in 0..150 {
            step_1x(&mut game);
            assert_eq!(game.leader_position(), at, "no step after the release");
        }
        assert!(down(&mut game, "ArrowDown"));
        let engine = game.engine.take().quiet();
        game.put_on_screen(engine);
        assert!(game.held.is_empty(), "a new game holds no key");
    }

    /// Spec 19 O1c (round 3 item 6): Ctrl or Alt down pauses a held walk and lets nothing go;
    /// the walk goes on once it is up.
    #[test]
    fn a_modifier_down_pauses_a_held_walk() {
        let Some(mut game) = in_the_castle() else {
            return;
        };
        assert!(down(&mut game, "ArrowDown"));
        let mut at = game.leader_position();
        let mut cells = 0;
        let mut count = |g: &Game, cells: &mut u32| {
            if g.leader_position() != at {
                at = g.leader_position();
                *cells += 1;
            }
        };
        for _ in 0..100 {
            step_1x(&mut game);
            count(&game, &mut cells);
        }
        assert!(cells >= 2, "walking");
        game.set_modifiers(true);
        let before = cells;
        for _ in 0..150 {
            step_1x(&mut game);
            count(&game, &mut cells);
        }
        assert!(cells <= before + 1, "paused (at most the turn in flight)");
        assert!(!game.held.is_empty(), "nothing let go");
        let paused = cells;
        game.set_modifiers(false);
        for _ in 0..100 {
            step_1x(&mut game);
            count(&game, &mut cells);
        }
        assert!(cells > paused, "walks on once it is up");
    }

    /// A game that stands for the waits: it reads one key, then waits 100 ms, over and over;
    /// the keys it read go to `out`. With `prompt` it is the command prompt (A4-0x7a2c and
    /// A4-0x7a42 set, [`u6_core::engine::command_prompt`]) and reads as the prompt does, the
    /// newest press queued by the drained take once a frame
    /// ([`u6_core::ui::keyread::take_drained`]: the platform's direction repeats taken back, the
    /// take counted); without, another wait such as the line editor's, reading in order with
    /// every repeat.
    fn reader(prompt: bool, out: Rc<std::cell::RefCell<Vec<u6_core::engine::Typed>>>) -> Engine {
        use u6_core::engine::Typed;
        let mut state = u6_core::State::empty();
        state.ui.input.accept = true;
        Engine::with_game(state, Files::default(), move |st, io| async move {
            loop {
                let t = if prompt {
                    st.borrow_mut().main.reading = 1;
                    loop {
                        if let Some(k) = u6_core::ui::keyread::take_drained(&st, &io) {
                            break Typed::Key(k);
                        }
                        io.vblank().await;
                    }
                } else {
                    io.next_typed().await
                };
                st.borrow_mut().main.reading = 0;
                out.borrow_mut().push(t);
                io.sleep(100).await;
            }
        })
    }

    /// Spec 19 O1c on [`reader`]: with the key `code` / `key` pressed (no browser repeat) and
    /// held for 1.6 s at 1×: how many keys it read.
    fn keys_read(prompt: bool, code: &str, key: &str) -> usize {
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut game = Game::new();
        game.engine = Some(reader(prompt, Rc::clone(&seen)));
        step_1x(&mut game);
        let at = game.clock;
        assert!(game.key_down(code, key, false, at));
        for _ in 0..80 {
            step_1x(&mut game);
        }
        seen.borrow().len()
    }

    /// Spec 19 O1c (round 3 item 6): on the page an arrow pressed with Ctrl down is no game
    /// key (the page's Alt and Meta chords never reach it either), so it never walks, as in the
    /// native window an arrow pressed with a modifier does not.
    #[test]
    fn an_arrow_pressed_with_ctrl_is_no_game_key_and_never_walks() {
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut game = Game::new();
        game.engine = Some(reader(true, Rc::clone(&seen)));
        step_1x(&mut game);
        let at = game.clock;
        assert!(!game.key_down("ArrowLeft", "ArrowLeft", true, at));
        for _ in 0..80 {
            step_1x(&mut game);
        }
        assert!(game.held.is_empty() && seen.borrow().is_empty());
    }

    /// Spec 19 O1c: at the command prompt a held arrow is pressed again at every read the
    /// minimum interval allows.
    #[test]
    fn a_held_arrow_at_the_command_prompt_is_pressed_at_every_read() {
        assert!(keys_read(true, "ArrowLeft", "ArrowLeft") >= 8);
    }

    /// Spec 19 O1c: a held letter at the command prompt is read once (only the browser's
    /// repeat would repeat it).
    #[test]
    fn a_held_letter_at_the_command_prompt_is_read_once() {
        assert_eq!(keys_read(true, "KeyX", "x"), 1);
    }

    /// Spec 19 O1c: a held arrow at any other wait (the line editor's) is read once.
    #[test]
    fn a_held_arrow_at_another_wait_is_read_once() {
        assert_eq!(keys_read(false, "ArrowLeft", "ArrowLeft"), 1);
    }

    /// Spec 19 O1c (review S2, round 3 item 3; round 4 item 7) through the page's tracking
    /// (`key_down`, `key_repeat`, `key_up`) on [`reader`] at the command prompt, every read
    /// checked against the oracle: a paced press is always the newest walking direction, and a
    /// read of an older direction only consumes a physical press queued before the newest press
    /// was made, at most once per direction change; the command prompt's drain (round 5: the
    /// reader reads as the prompt does) takes the newest of those, so here none is. Phase A:
    /// the down arrow held alone reads only Down. Phase B: Down let go and pressed again, then
    /// Left pressed, in one frame, both presses queued: the drain takes Left and drops that Down
    /// press, so only Left.
    /// Phase C: the browser's repeat of Down is taken back at the prompt, so only Left (a repeat
    /// does not make the older newer). Phase D: Left let go, only Down. Each phase lasts 400 ms
    /// at 1×.
    #[test]
    fn the_newest_held_arrow_walks_and_the_older_comes_back_at_its_release() {
        use u6_core::engine::Typed;
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut game = Game::new();
        game.engine = Some(reader(true, Rc::clone(&seen)));
        step_1x(&mut game);
        let phase = |game: &mut Game| {
            let from = seen.borrow().len();
            for _ in 0..20 {
                step_1x(game);
            }
            seen.borrow()[from..].to_vec()
        };
        let (south, west) = (Typed::Key(Key::Down), Typed::Key(Key::Left));
        assert!(down(&mut game, "ArrowDown"));
        let a = phase(&mut game);
        assert!(
            a.len() >= 2 && a.iter().all(|&t| t == south),
            "Down alone: {a:?}"
        );
        assert!(game.key_up("ArrowDown"));
        assert!(down(&mut game, "ArrowDown"));
        assert!(down(&mut game, "ArrowLeft"));
        // Round 6 (review opus N3): the page queued both presses (after Down's release), so the
        // drain below has the Down press to drop.
        let presses: Vec<InputEvent> = game
            .pending
            .iter()
            .copied()
            .filter(|e| matches!(e, InputEvent::Key(_)))
            .collect();
        assert_eq!(
            presses,
            [InputEvent::Key(Key::Down), InputEvent::Key(Key::Left)],
            "both presses queued"
        );
        let b = phase(&mut game);
        assert!(
            b.len() >= 2 && b.iter().all(|&t| t == west),
            "the Down press queued before Left's drained, only the newest: {b:?}"
        );
        assert!(repeat(&mut game, "ArrowDown"));
        let c = phase(&mut game);
        assert!(
            c.len() >= 2 && c.iter().all(|&t| t == west),
            "the browser's repeat of Down taken back, only Left: {c:?}"
        );
        assert!(game.key_up("ArrowLeft"));
        let d = phase(&mut game);
        assert!(
            d.len() >= 2 && d.iter().all(|&t| t == south),
            "the older again: {d:?}"
        );
    }

    /// Spec 19 O1c (round 4 item 2): an arrow whose press was not the game's never walks,
    /// though its repeats reach the page with no modifier down, through the page's calls in the
    /// order the page makes them: Alt down (the chord is not the game's: the page sets the
    /// modifiers, [`Game::set_modifiers`], and calls no `key_down`), the arrow pressed (no
    /// `key_down` either), Alt let go, then 1 s of the browser's repeats of the arrow (one every
    /// 30 ms); the same with Ctrl, where the arrow pressed with Ctrl down is no game key. No
    /// step in the castle: the repeats are taken back at the command prompt, and the key, first
    /// seen by a repeat, has no paced press.
    #[test]
    fn an_arrow_first_pressed_with_alt_or_ctrl_never_walks() {
        for ctrl in [false, true] {
            let Some(mut game) = in_the_castle() else {
                return;
            };
            let mut at = game.leader_position();
            game.set_modifiers(true);
            if ctrl {
                let t = game.clock;
                assert!(
                    !game.key_down("ArrowDown", "ArrowDown", true, t),
                    "no game key"
                );
            }
            game.set_modifiers(false);
            let t0 = game.clock;
            let mut next = 0.0;
            let mut steps = 0;
            while game.clock - t0 < 1000.0 {
                while game.clock - t0 + 20.0 > next {
                    assert!(game.key_repeat("ArrowDown", "ArrowDown", false, t0 + next));
                    next += 30.0;
                }
                step_1x(&mut game);
                if game.leader_position() != at {
                    at = game.leader_position();
                    steps += 1;
                }
            }
            let what = if ctrl { "Ctrl" } else { "Alt" };
            println!("an arrow first pressed with {what}: {steps} steps in 1 s of its repeats");
            assert_eq!(steps, 0, "first pressed with {what}: no step");
            assert!(game.held.iter().all(|h| !h.walking), "held, not walking");
            assert!(game.key_up("ArrowDown"));
        }
    }

    /// Spec 19 O1c (round 4 item 2): the repeats of a key that does not walk (first seen by a
    /// repeat) still reach every wait but the command prompt: at another wait ([`reader`]
    /// without the prompt: the line editor's, say) each is read.
    #[test]
    fn the_repeats_of_a_key_that_does_not_walk_reach_another_wait() {
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut game = Game::new();
        game.engine = Some(reader(false, Rc::clone(&seen)));
        step_1x(&mut game);
        for _ in 0..4 {
            let at = game.clock;
            assert!(game.key_repeat("ArrowLeft", "ArrowLeft", false, at));
            for _ in 0..8 {
                step_1x(&mut game);
            }
        }
        assert_eq!(seen.borrow().len(), 4, "each repeat read");
        assert!(game.held.iter().all(|h| !h.walking));
    }
}

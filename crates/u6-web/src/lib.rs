//! Browser entry point: collects the dropped files, runs u6-intake, boots `u6_core::Engine`
//! and hands frames back as RGBA8 (see `site/app.js`).
//!
//! JS owns the clock and the DOM. Key callbacks only queue events; [`Game::tick`] delivers them
//! to the engine with the elapsed time, so game code never runs inside an input handler.
//!
//! Limits (a clear message instead of an exhausted tab): [`MAX_INPUT_BYTES`] for the dropped
//! files together, [`MAX_UNZIPPED_BYTES`] for what the zip entries the intake inflates declare,
//! [`MAX_GAME_FILES_BYTES`] for the game files the intake produces.

mod zipscan;

use u6_core::{Engine, Files, Framebuffer, InputEvent, Key};
use u6_intake::{InputFile, intake};
use wasm_bindgen::prelude::*;

/// The dropped files together (the archive.org zip is 185 MiB, the game three 880 KiB disks).
/// The page checks the file sizes before reading anything; [`Game::load_file`] is the backstop.
pub const MAX_INPUT_BYTES: usize = 512 << 20;
/// What the disk-image entries of the dropped zips (the only entries the intake inflates)
/// declare uncompressed, together; read from the central directories before anything is
/// inflated (u6-intake caps each inflate at its declared size).
pub const MAX_UNZIPPED_BYTES: u64 = 64 << 20;
/// The game files the intake produced, together (the Amiga disks hold 2.4 MiB), checked before
/// the engine boots. PP20 output is bounded per file by its 24-bit length field (16 MiB), not in
/// aggregate, inside u6-intake.
pub const MAX_GAME_FILES_BYTES: usize = 64 << 20;

/// [`MAX_INPUT_BYTES`] for the page.
#[wasm_bindgen]
pub fn max_input_bytes() -> f64 {
    MAX_INPUT_BYTES as f64
}

/// The message for inputs over [`MAX_INPUT_BYTES`].
pub fn too_large(total: u64) -> String {
    format!(
        "The dropped files are {} MiB together; this page takes at most {} MiB \
         (the archive.org zip, the three disks or the extracted files).",
        total >> 20,
        MAX_INPUT_BYTES >> 20
    )
}

/// Maps a DOM `KeyboardEvent` (`code`, `key`) to an engine key: named keys by `code`,
/// printable single characters by `key`.
pub fn key_from_dom(code: &str, key: &str) -> Option<Key> {
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
    /// Bytes in `inputs`.
    input_bytes: usize,
    engine: Option<Engine>,
    pending: Vec<InputEvent>,
    blank: Framebuffer,
    rgba: Vec<u8>,
    log: Vec<String>,
    /// Paths of the game files the last start booted from, until the page has kept them (their
    /// bytes are read in place from the engine's file map, never copied in wasm).
    kept: Vec<String>,
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
            engine: None,
            pending: Vec::new(),
            blank: Framebuffer::new(),
            rgba: Vec::new(),
            log: Vec::new(),
            kept: Vec::new(),
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

    /// Runs the intake over the collected files and boots the engine with RNG seed `seed` (the
    /// original seeds from the clock; the page passes the low word of the time). Returns the
    /// intake report, one line per entry; fails with the report plus the boot error.
    pub fn start(&mut self, seed: u16) -> Result<String, JsError> {
        self.boot(seed).map_err(|report| JsError::new(&report))
    }

    /// Number of game files the running game booted from.
    pub fn game_files(&self) -> u32 {
        self.engine
            .as_ref()
            .map_or(0, |e| e.io().files().len() as u32)
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
        }
        self.pending.clear();
        self.redraw();
    }

    /// Queues a key press. Returns true when it is a game key (the caller should then
    /// `preventDefault` so arrows do not scroll the page).
    pub fn key_down(&mut self, code: &str, key: &str) -> bool {
        match key_from_dom(code, key) {
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

    /// Kept file `i`, read in place from the running engine's files.
    fn kept_bytes(&self, i: u32) -> Option<&[u8]> {
        let path = self.kept.get(i as usize)?;
        self.engine.as_ref()?.io().files().get(path)
    }

    /// [`Game::start`] without the JS error type (testable off wasm).
    fn boot(&mut self, seed: u16) -> Result<String, String> {
        let inputs = std::mem::take(&mut self.inputs);
        self.input_bytes = 0;
        let unzipped: u64 = inputs
            .iter()
            .filter_map(|f| zipscan::declared_disk_bytes(&f.data))
            .sum();
        if unzipped > MAX_UNZIPPED_BYTES {
            return Err(format!(
                "The disk images in the dropped zip declare {} MiB unpacked; this page unpacks \
                 at most {} MiB.",
                unzipped >> 20,
                MAX_UNZIPPED_BYTES >> 20
            ));
        }
        let r = intake(inputs);
        drop(r.disks);
        let mut report = r.report.join("\n");
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
        match Engine::with_seed(Files::new(r.files), seed) {
            Ok(engine) => {
                // A path that differs from another only in case is shadowed (the game's lookups
                // ignore case): the engine never reads it, so it is not kept either.
                let files = engine.io().files();
                paths.retain(|p| files.resolve(p).is_some_and(|(_, key)| key == p));
                self.kept = paths;
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
        assert_eq!(key_from_dom("ArrowRight", "ArrowRight"), Some(Key::Right));
        assert_eq!(key_from_dom("NumpadEnter", "Enter"), Some(Key::Enter));
        assert_eq!(key_from_dom("KeyT", "t"), Some(Key::Char('t')));
        assert_eq!(key_from_dom("KeyT", "T"), Some(Key::Char('T')));
        assert_eq!(key_from_dom("ShiftLeft", "Shift"), None);
        assert_eq!(key_from_dom("F1", "F1"), None);
    }

    #[test]
    fn before_start_the_frame_is_black_and_keys_are_not_queued() {
        let mut game = Game::new();
        assert_eq!(game.frame_len(), 320 * 200 * 4);
        assert!(game.frame_rgba().chunks(4).all(|p| p == [0, 0, 0, 255]));
        assert!(game.key_down("ArrowUp", "ArrowUp"));
        assert!(!game.key_down("ShiftLeft", "Shift"));
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
        let report = game.boot(0).unwrap_err();
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
        let data = std::fs::read(ZIP).unwrap();
        let unzipped = zipscan::declared_disk_bytes(&data).unwrap();
        assert!(unzipped > 0 && unzipped < MAX_UNZIPPED_BYTES, "{unzipped}");
        let mut game = Game::new();
        game.collect("ultima-vi.zip", data).unwrap();
        game.boot(1).unwrap();
        let n = game.kept_count();
        assert_eq!(n, game.game_files());
        let kept: Vec<(String, Vec<u8>)> = (0..n)
            .map(|i| (game.kept_path(i), game.kept_bytes(i).unwrap().to_vec()))
            .collect();
        assert!(kept.iter().all(|(_, b)| !b.is_empty()));
        game.release_kept();
        assert_eq!((game.kept_count(), game.kept_len(0)), (0, 0));
        let mut again = Game::new();
        for (path, bytes) in kept {
            again.collect(&path, bytes).unwrap();
        }
        again.boot(1).unwrap();
        assert_eq!(again.game_files(), n);
        for _ in 0..60 {
            game.tick(16);
            again.tick(16);
        }
        assert!(game.frame_rgba() == again.frame_rgba(), "same first frames");
    }
}

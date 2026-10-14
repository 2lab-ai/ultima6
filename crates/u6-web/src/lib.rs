//! Browser entry point: collects the dropped files, runs u6-intake, boots `u6_core::Engine`
//! and hands frames back as RGBA8 (see `site/app.js`).
//!
//! JS owns the clock and the DOM. Key callbacks only queue events; [`Game::tick`] delivers them
//! to the engine with the elapsed time, so game code never runs inside an input handler.

use u6_core::{Engine, Files, Framebuffer, InputEvent, Key};
use u6_intake::{InputFile, intake};
use wasm_bindgen::prelude::*;

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
    engine: Option<Engine>,
    pending: Vec<InputEvent>,
    blank: Framebuffer,
    rgba: Vec<u8>,
    log: Vec<String>,
    /// The game files the intake extracted at the last start, until the page has stored them.
    extracted: Vec<(String, Vec<u8>)>,
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
            engine: None,
            pending: Vec::new(),
            blank: Framebuffer::new(),
            rgba: Vec::new(),
            log: Vec::new(),
            extracted: Vec::new(),
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
    pub fn load_file(&mut self, path: &str, bytes: &[u8]) {
        self.inputs.push(InputFile {
            name: path.to_string(),
            data: bytes.to_vec(),
        });
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

    /// Number of game files the last start extracted (the page stores them so a reload needs
    /// no new drop; each is a loose file the intake passes through).
    pub fn extracted_count(&self) -> u32 {
        self.extracted.len() as u32
    }

    /// Path of extracted file `i` (`VOLUME/dir/file`).
    pub fn extracted_path(&self, i: u32) -> String {
        self.extracted
            .get(i as usize)
            .map_or_else(String::new, |(p, _)| p.clone())
    }

    /// Bytes of extracted file `i` (a copy).
    pub fn extracted_bytes(&self, i: u32) -> Vec<u8> {
        self.extracted
            .get(i as usize)
            .map_or_else(Vec::new, |(_, b)| b.clone())
    }

    /// Releases the extracted copies once stored.
    pub fn drop_extracted(&mut self) {
        self.extracted = Vec::new();
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
    /// [`Game::start`] without the JS error type (testable off wasm).
    fn boot(&mut self, seed: u16) -> Result<String, String> {
        let r = intake(std::mem::take(&mut self.inputs));
        let mut report = r.report.join("\n");
        let files = r.files;
        let copy: Vec<(String, Vec<u8>)> = files
            .iter()
            .map(|(p, b)| (p.clone(), b.clone()))
            .collect();
        match Engine::with_seed(Files::new(files), seed) {
            Ok(engine) => {
                self.extracted = copy;
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn start_without_game_files_reports_the_missing_file() {
        let mut game = Game::new();
        game.load_file("notes.txt", b"hello");
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
}

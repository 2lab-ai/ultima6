//! Browser entry point: wraps `u6_core::Game` for JavaScript (see `site/app.js`).
//!
//! JS owns the clock and the DOM; this crate only translates DOM key codes into
//! `u6_core::Key`, keeps the held-key state and hands frames back as RGBA8.

use u6_core::{Framebuffer, Input, Key};
use wasm_bindgen::prelude::*;

/// Maps a DOM `KeyboardEvent.code` to an engine key.
pub fn key_from_code(code: &str) -> Option<Key> {
    Some(match code {
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        "Enter" | "NumpadEnter" => Key::Enter,
        "Escape" => Key::Escape,
        "Space" => Key::Space,
        _ => return None,
    })
}

#[wasm_bindgen]
pub struct Game {
    core: u6_core::Game,
    input: Input,
    fb: Framebuffer,
    rgba: Vec<u8>,
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
            core: u6_core::Game::new(),
            input: Input::default(),
            fb: Framebuffer::new(),
            rgba: Vec::new(),
        };
        game.redraw();
        game
    }

    pub fn width(&self) -> u32 {
        self.fb.width() as u32
    }

    pub fn height(&self) -> u32 {
        self.fb.height() as u32
    }

    /// Advances the game by `ms` milliseconds and redraws the frame.
    pub fn tick(&mut self, ms: u32) {
        self.core.tick(&self.input, ms);
        self.redraw();
    }

    /// Returns true when `code` is a game key (the caller should then
    /// `preventDefault` so arrows do not scroll the page).
    pub fn key_down(&mut self, code: &str) -> bool {
        self.set_key(code, true)
    }

    pub fn key_up(&mut self, code: &str) -> bool {
        self.set_key(code, false)
    }

    /// Releases every key (call on window blur).
    pub fn release_all(&mut self) {
        self.input.clear();
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

    /// Stores a user-supplied file in the engine's virtual file system.
    pub fn load_file(&mut self, path: &str, bytes: &[u8]) {
        self.core.load_file(path, bytes.to_vec());
    }

    pub fn files_loaded(&self) -> u32 {
        self.core.files_loaded() as u32
    }
}

impl Game {
    fn set_key(&mut self, code: &str, down: bool) -> bool {
        match key_from_code(code) {
            Some(key) => {
                self.input.set(key, down);
                true
            }
            None => false,
        }
    }

    fn redraw(&mut self) {
        self.core.render(&mut self.fb);
        self.fb.write_rgba(&mut self.rgba);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_arrow_codes_and_rejects_others() {
        assert_eq!(key_from_code("ArrowRight"), Some(Key::Right));
        assert_eq!(key_from_code("NumpadEnter"), Some(Key::Enter));
        assert_eq!(key_from_code("KeyA"), None);
    }

    #[test]
    fn held_key_moves_marker_in_frame() {
        let mut game = Game::new();
        assert_eq!(game.frame_len(), 320 * 200 * 4);
        let white_at = |g: &Game, x: usize, y: usize| {
            let i = (y * 320 + x) * 4;
            g.frame_rgba()[i..i + 4] == [255, 255, 255, 255]
        };
        assert!(white_at(&game, 152, 92));
        assert!(game.key_down("ArrowRight"));
        assert!(!game.key_down("KeyQ"));
        game.tick(1000);
        assert!(game.key_up("ArrowRight"));
        game.tick(1000);
        assert!(!white_at(&game, 152, 92));
        assert!(white_at(&game, 212, 92));
    }

    #[test]
    fn load_file_counts_distinct_paths() {
        let mut game = Game::new();
        game.load_file("a", &[1]);
        game.load_file("b", &[]);
        game.load_file("a", &[2]);
        assert_eq!(game.files_loaded(), 2);
    }
}

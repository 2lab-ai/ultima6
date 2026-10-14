//! Platform-free Ultima VI game state and logic.
//!
//! Contract (every platform and every later layer — AI NPC drivers, mods — relies on it):
//!
//! - **In:** the platform calls [`Game::load_file`] with bytes the user supplied, then
//!   once per frame [`Game::tick`] with the current [`Input`] (keys held) and the
//!   milliseconds elapsed since the previous tick.
//! - **Out:** [`Game::render`] draws palette indices and the palette into a
//!   [`Framebuffer`]; the platform converts and presents it.
//! - This crate never performs I/O and never reads a clock: no files, network,
//!   environment, threads, `std::time`. Given the same files, inputs and tick lengths it
//!   produces the same frames, so runs are replayable and testable headless.
//!   `clippy.toml` in this crate turns the common violations into lint errors.
//!
//! Current state: skeleton. `render` draws a test pattern (palette ramp strip plus a
//! 16x16 marker the arrow keys move) so input → tick → frame is visible end to end.

use std::collections::BTreeMap;

pub use u6_render::{Framebuffer, HEIGHT, Rgb, WIDTH};

/// Logical keys the engine understands. Platforms map their key codes onto these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Space,
}

impl Key {
    pub const ALL: [Key; 7] = [
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
        Key::Enter,
        Key::Escape,
        Key::Space,
    ];

    const fn bit(self) -> u32 {
        1 << self as u8
    }
}

/// Keys currently held, as the platform last reported them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    held: u32,
}

impl Input {
    pub fn set(&mut self, key: Key, down: bool) {
        if down {
            self.held |= key.bit();
        } else {
            self.held &= !key.bit();
        }
    }

    pub fn is_down(&self, key: Key) -> bool {
        self.held & key.bit() != 0
    }

    /// Releases every key (e.g. when the window loses focus).
    pub fn clear(&mut self) {
        self.held = 0;
    }
}

/// Side of the test-pattern marker in pixels.
pub const MARKER_SIZE: usize = 16;
/// Marker speed in pixels per second.
pub const MARKER_SPEED: i64 = 60;
/// Palette index of the marker (white in the test palette).
pub const MARKER_INDEX: u8 = 63;
/// Rows of the palette ramp strip at the top of the test pattern.
pub const RAMP_ROWS: usize = 16;

const MARKER_MAX_X: i64 = ((WIDTH - MARKER_SIZE) * 1000) as i64;
const MARKER_MAX_Y: i64 = ((HEIGHT - MARKER_SIZE) * 1000) as i64;

pub struct Game {
    vfs: BTreeMap<String, Vec<u8>>,
    /// Marker position in thousandths of a pixel.
    marker_milli: (i64, i64),
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        Self {
            vfs: BTreeMap::new(),
            marker_milli: (MARKER_MAX_X / 2, MARKER_MAX_Y / 2),
        }
    }

    /// Stores a user-supplied file under `path` (kept verbatim), replacing any
    /// previous file with the same path.
    pub fn load_file(&mut self, path: impl Into<String>, bytes: Vec<u8>) {
        self.vfs.insert(path.into(), bytes);
    }

    pub fn files_loaded(&self) -> usize {
        self.vfs.len()
    }

    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.vfs.get(path).map(Vec::as_slice)
    }

    /// Top-left corner of the marker in whole pixels.
    pub fn marker_pos(&self) -> (i32, i32) {
        (
            (self.marker_milli.0 / 1000) as i32,
            (self.marker_milli.1 / 1000) as i32,
        )
    }

    /// Advances the game by `ms` milliseconds with `input` held throughout.
    pub fn tick(&mut self, input: &Input, ms: u32) {
        let axis =
            |neg: Key, pos: Key| i64::from(input.is_down(pos)) - i64::from(input.is_down(neg));
        let step = MARKER_SPEED * i64::from(ms);
        let (x, y) = &mut self.marker_milli;
        *x = (*x + axis(Key::Left, Key::Right) * step).clamp(0, MARKER_MAX_X);
        *y = (*y + axis(Key::Up, Key::Down) * step).clamp(0, MARKER_MAX_Y);
    }

    /// Draws the current frame (palette included) into `fb`.
    pub fn render(&self, fb: &mut Framebuffer) {
        for (i, entry) in fb.palette.iter_mut().enumerate() {
            let level = ((i % 64) * 255 / 63) as u8;
            *entry = match i / 64 {
                0 => Rgb::new(level, level, level),
                1 => Rgb::new(level, 0, 0),
                2 => Rgb::new(0, level, 0),
                _ => Rgb::new(0, 0, level),
            };
        }
        fb.clear(0);
        for y in 0..RAMP_ROWS {
            for x in 0..WIDTH {
                fb.set(x, y, (x * 256 / WIDTH) as u8);
            }
        }
        let (x, y) = self.marker_pos();
        let marker = [MARKER_INDEX; MARKER_SIZE * MARKER_SIZE];
        fb.blit(x, y, MARKER_SIZE, MARKER_SIZE, &marker, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(keys: &[Key]) -> Input {
        let mut input = Input::default();
        for &k in keys {
            input.set(k, true);
        }
        input
    }

    #[test]
    fn input_tracks_each_key_independently() {
        let mut input = hold(&Key::ALL);
        input.set(Key::Left, false);
        for k in Key::ALL {
            assert_eq!(input.is_down(k), k != Key::Left, "{k:?}");
        }
        input.clear();
        assert!(Key::ALL.iter().all(|&k| !input.is_down(k)));
    }

    #[test]
    fn holding_right_moves_marker_right_only() {
        let mut game = Game::new();
        let start = game.marker_pos();
        assert_eq!(start, (152, 92));
        for _ in 0..60 {
            game.tick(&hold(&[Key::Right]), 16);
        }
        // 60 ticks * 16 ms = 960 ms at 60 px/s = 57.6 px.
        assert_eq!(game.marker_pos(), (209, 92));
    }

    #[test]
    fn opposite_keys_cancel_and_marker_stays_on_screen() {
        let mut game = Game::new();
        game.tick(&hold(&[Key::Left, Key::Right]), 1000);
        assert_eq!(game.marker_pos(), (152, 92));
        game.tick(&hold(&[Key::Up, Key::Left]), u32::MAX);
        assert_eq!(game.marker_pos(), (0, 0));
        game.tick(&hold(&[Key::Down, Key::Right]), u32::MAX);
        assert_eq!(
            game.marker_pos(),
            ((WIDTH - MARKER_SIZE) as i32, (HEIGHT - MARKER_SIZE) as i32)
        );
    }

    #[test]
    fn render_draws_ramp_and_marker() {
        let mut game = Game::new();
        game.tick(&hold(&[Key::Right]), 1000);
        let mut fb = Framebuffer::new();
        game.render(&mut fb);
        let (x, y) = game.marker_pos();
        assert_eq!((x, y), (212, 92));
        assert_eq!(fb.get(x as usize, y as usize), Some(MARKER_INDEX));
        assert_eq!(fb.get(x as usize + 15, y as usize + 15), Some(MARKER_INDEX));
        assert_eq!(fb.get(x as usize - 1, y as usize), Some(0));
        assert_eq!(fb.get(0, 0), Some(0));
        assert_eq!(fb.get(WIDTH - 1, 0), Some(255));
        assert_eq!(fb.palette[MARKER_INDEX as usize], Rgb::new(255, 255, 255));
    }

    #[test]
    fn vfs_stores_files_verbatim_and_replaces_same_path() {
        let mut game = Game::new();
        game.load_file("DATA1/map", vec![1, 2]);
        game.load_file("ultima-vi.zip", vec![3]);
        game.load_file("DATA1/map", vec![4]);
        assert_eq!(game.files_loaded(), 2);
        assert_eq!(game.file("DATA1/map"), Some(&[4u8][..]));
        assert_eq!(game.file("data1/map"), None);
    }
}

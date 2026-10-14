//! Desktop runner: boots the engine from the user's archive.org zip, then either opens a
//! 3x-scaled window or renders headless to PNG for pixel checks.
//!
//! ```text
//! u6-native --zip ultima-vi.zip [--seed N] [--save-dir DIR]      open the window
//! u6-native --zip ultima-vi.zip --headless-png out.png [--frames N] [--keys up,up,left,t] [--seed N] [--save-dir DIR]
//! ```
//!
//! `--save-dir DIR` keeps the game's saves in `DIR/savegame/` (the original's PLAYER-disk
//! layout; an original `savegame/` directory copied there loads as it is): the boot reads the
//! saved state from there, and every Save the game makes is written there after its tick.
//! Without it nothing is stored and every boot starts from the zip's new-game files.
//!
//! Headless mode never opens a window or reads a clock: it runs N ticks of `HEADLESS_TICK_MS`,
//! delivering the `--keys` one per tick while the game has no key queued, and writes the last
//! published 320x200 frame. A `say:words` token types `words` and Return into the next line
//! input, pressing space (one at a time, after the game has idled a few ticks) for every more
//! prompt or key wait before it. The RNG seed defaults to 0 headless (reproducible) and to the
//! wall clock in the window (as the original seeds from `time()`).

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use minifb::{KeyRepeat, Window, WindowOptions};
use u6_core::saveload::{SaveSync, attach};
use u6_core::{Engine, Files, HEIGHT, InputEvent, Key, WIDTH};
use u6_intake::{InputFile, intake};

mod saves;
use saves::DirStore;

const SCALE: usize = 3;
const HEADLESS_TICK_MS: u32 = 16;
const MAX_STEP_MS: u64 = 100;

#[derive(Debug, Default, PartialEq)]
struct Args {
    zip: Option<PathBuf>,
    headless_png: Option<PathBuf>,
    frames: u32,
    keys: Vec<Token>,
    seed: Option<u16>,
    save_dir: Option<PathBuf>,
}

/// One `--keys` token.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Key(Key),
    /// Text typed into the next line input, then Return.
    Say(String),
}

fn parse_token(token: &str) -> Result<Token, String> {
    match token.strip_prefix("say:") {
        Some(text) => Ok(Token::Say(text.to_string())),
        None => parse_key(token).map(Token::Key),
    }
}

/// One key token: an arrow / control key name or a single character.
fn parse_key(token: &str) -> Result<Key, String> {
    Ok(match token.to_ascii_lowercase().as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        _ => {
            let mut chars = token.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Key::Char(c),
                _ => return Err(format!("unknown key {token:?}")),
            }
        }
    })
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        frames: 60,
        ..Args::default()
    };
    while let Some(arg) = args.next() {
        let mut value = |what: &str| args.next().ok_or(format!("{arg} needs {what}"));
        match arg.as_str() {
            "--zip" => parsed.zip = Some(value("a path")?.into()),
            "--headless-png" => parsed.headless_png = Some(value("a path")?.into()),
            "--frames" => {
                let n = value("a number")?;
                parsed.frames = n.parse().map_err(|_| format!("bad --frames {n:?}"))?;
            }
            "--keys" => {
                let seq = value("a key list")?;
                parsed.keys = seq
                    .split(',')
                    .filter(|t| !t.is_empty())
                    .map(parse_token)
                    .collect::<Result<_, _>>()?;
            }
            "--seed" => {
                let n = value("a number")?;
                parsed.seed = Some(n.parse().map_err(|_| format!("bad --seed {n:?}"))?);
            }
            "--save-dir" => parsed.save_dir = Some(value("a directory")?.into()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(parsed)
}

/// Where saves go, if anywhere: the store and what it has handed over so far.
struct Saves {
    store: DirStore,
    sync: SaveSync,
}

impl Saves {
    /// After a tick: writes what a Save in it committed.
    fn flush(&mut self, engine: &Engine) -> Result<(), String> {
        let n = self
            .sync
            .flush(engine, &mut self.store)
            .map_err(|e| e.to_string())?;
        if n > 0 {
            eprintln!("save: wrote {n} files");
        }
        Ok(())
    }
}

/// Reads the zip, runs the intake (report lines to stderr), lays the stored save over the
/// files and boots the engine.
fn boot(zip: &Path, seed: u16, saves: Option<&Saves>) -> Result<Engine, String> {
    let data = std::fs::read(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let name = zip
        .file_name()
        .map_or("input.zip".into(), |n| n.to_string_lossy().into_owned());
    let r = intake(vec![InputFile { name, data }]);
    for line in &r.report {
        eprintln!("intake: {line}");
    }
    let mut files = Files::new(r.files);
    if let Some(saves) = saves {
        let n = attach(&mut files, &saves.store).map_err(|e| e.to_string())?;
        eprintln!("save: {n} stored files over the new-game files");
    }
    Engine::with_seed(files, seed).map_err(|e| e.to_string())
}

fn print_log(engine: &mut Engine) {
    for line in engine.take_log() {
        eprintln!("{line}");
    }
}

/// Ticks the game must idle with nothing queued before a space answers a wait for `say:`.
const IDLE_TICKS: u32 = 3;

/// Runs `frames` fixed ticks feeding the tokens (see the module doc); returns the last frame as
/// RGBA8.
fn simulate(
    engine: &mut Engine,
    frames: u32,
    tokens: &[Token],
    mut saves: Option<&mut Saves>,
) -> Result<Vec<u8>, String> {
    let mut queue: std::collections::VecDeque<Key> = std::collections::VecDeque::new();
    let mut next = tokens.iter();
    let mut pending: Option<String> = None;
    let mut idle = 0u32;
    for _ in 0..frames {
        let busy = engine.io().pending_events() > 0;
        idle = if busy { 0 } else { idle + 1 };
        let mut input = Vec::new();
        if !busy {
            if let Some(k) = queue.pop_front() {
                input.push(InputEvent::Key(k));
            } else if let Some(text) = pending.take() {
                if engine.state().ui.input.line_input {
                    queue.extend(text.chars().map(Key::Char));
                    queue.push_back(Key::Enter);
                    if let Some(k) = queue.pop_front() {
                        input.push(InputEvent::Key(k));
                    }
                } else {
                    if idle >= IDLE_TICKS {
                        input.push(InputEvent::Key(Key::Space));
                        idle = 0;
                    }
                    pending = Some(text);
                }
            } else {
                match next.next() {
                    Some(Token::Key(k)) => input.push(InputEvent::Key(*k)),
                    Some(Token::Say(text)) => pending = Some(text.clone()),
                    None => {}
                }
            }
        }
        engine.tick(&input, HEADLESS_TICK_MS);
        if let Some(saves) = saves.as_deref_mut() {
            saves.flush(engine)?;
        }
    }
    let mut rgba = Vec::new();
    engine.frame().write_rgba(&mut rgba);
    Ok(rgba)
}

fn write_png(path: &Path, rgba: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), WIDTH as u32, HEIGHT as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())
}

/// Nearest-neighbour SCALE x upscale of an RGBA8 frame into minifb's 0RGB pixels.
fn upscale(rgba: &[u8], out: &mut [u32]) {
    let out_w = WIDTH * SCALE;
    for (y, row) in rgba.as_chunks::<{ WIDTH * 4 }>().0.iter().enumerate() {
        let first = y * SCALE * out_w;
        for (x, px) in row.as_chunks::<4>().0.iter().enumerate() {
            let rgb = u32::from(px[0]) << 16 | u32::from(px[1]) << 8 | u32::from(px[2]);
            out[first + x * SCALE..first + (x + 1) * SCALE].fill(rgb);
        }
        for copy in 1..SCALE {
            out.copy_within(first..first + out_w, first + copy * out_w);
        }
    }
}

fn map_key(key: minifb::Key) -> Option<Key> {
    use minifb::Key as K;
    let letter = |k: K| {
        let i = k as u8 - K::A as u8;
        Key::Char(char::from(b'a' + i))
    };
    Some(match key {
        K::Up => Key::Up,
        K::Down => Key::Down,
        K::Left => Key::Left,
        K::Right => Key::Right,
        K::Enter | K::NumPadEnter => Key::Enter,
        K::Escape => Key::Escape,
        K::Space => Key::Space,
        K::Backspace => Key::Backspace,
        K::Key0 => Key::Char('0'),
        K::Key1 => Key::Char('1'),
        K::Key2 => Key::Char('2'),
        K::Key3 => Key::Char('3'),
        K::Key4 => Key::Char('4'),
        K::Key5 => Key::Char('5'),
        K::Key6 => Key::Char('6'),
        K::Key7 => Key::Char('7'),
        K::Key8 => Key::Char('8'),
        K::Key9 => Key::Char('9'),
        k if (K::A as u8..=K::Z as u8).contains(&(k as u8)) => letter(k),
        _ => return None,
    })
}

fn run_window(mut engine: Engine, mut saves: Option<Saves>) -> Result<(), String> {
    let (w, h) = (WIDTH * SCALE, HEIGHT * SCALE);
    let mut window =
        Window::new("Ultima VI", w, h, WindowOptions::default()).map_err(|e| e.to_string())?;
    window.set_target_fps(60);
    let mut rgba = Vec::new();
    let mut screen = vec![0u32; w * h];
    let start = Instant::now();
    let mut delivered_ms = 0u64;
    while window.is_open() {
        let input: Vec<InputEvent> = window
            .get_keys_pressed(KeyRepeat::Yes)
            .into_iter()
            .filter_map(map_key)
            .map(InputEvent::Key)
            .collect();
        let now_ms = start.elapsed().as_millis() as u64;
        let step = (now_ms - delivered_ms).min(MAX_STEP_MS);
        delivered_ms = now_ms;
        engine.tick(&input, step as u32);
        if let Some(saves) = saves.as_mut() {
            saves.flush(&engine)?;
        }
        print_log(&mut engine);
        engine.frame().write_rgba(&mut rgba);
        upscale(&rgba, &mut screen);
        window
            .update_with_buffer(&screen, w, h)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Low word of the wall-clock seconds, like the original's `time()` seed.
fn clock_seed() -> u16 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u16)
}

fn run(args: Args) -> Result<(), String> {
    let zip = args
        .zip
        .as_deref()
        .ok_or("--zip <ultima-vi.zip> is required")?;
    let mut saves = args.save_dir.as_ref().map(|dir| Saves {
        store: DirStore::new(dir),
        sync: SaveSync::default(),
    });
    match &args.headless_png {
        Some(path) => {
            let mut engine = boot(zip, args.seed.unwrap_or(0), saves.as_ref())?;
            let rgba = simulate(&mut engine, args.frames, &args.keys, saves.as_mut())?;
            print_log(&mut engine);
            write_png(path, &rgba)?;
            let hits = engine.stub_hits();
            let stubs: u64 = hits.values().sum();
            println!(
                "wrote {} ({} frames, {} keys still queued, {} stub hits)",
                path.display(),
                args.frames,
                engine.io().pending_events(),
                stubs
            );
            for (id, n) in hits {
                println!("  {n:6} {id}");
            }
            if std::env::var_os("U6_TRANSCRIPT").is_some() {
                for line in &engine.state().ui.messages {
                    println!("| {line}");
                }
            }
            Ok(())
        }
        None => {
            let engine = boot(zip, args.seed.unwrap_or_else(clock_seed), saves.as_ref())?;
            run_window(engine, saves)
        }
    }
}

fn main() -> ExitCode {
    match parse_args(std::env::args().skip(1)).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("u6-native: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_flags_and_key_lists() {
        let a = args(&[
            "--zip",
            "u6.zip",
            "--headless-png",
            "o.png",
            "--frames",
            "5",
            "--keys",
            "up,Right,t,ESC",
            "--seed",
            "77",
        ])
        .unwrap();
        assert_eq!(a.zip, Some(PathBuf::from("u6.zip")));
        assert_eq!(a.headless_png, Some(PathBuf::from("o.png")));
        assert_eq!((a.frames, a.seed), (5, Some(77)));
        assert_eq!(a.save_dir, None);
        assert_eq!(
            args(&["--save-dir", "saves"]).unwrap().save_dir,
            Some(PathBuf::from("saves"))
        );
        assert!(args(&["--save-dir"]).is_err());
        assert_eq!(
            a.keys,
            [Key::Up, Key::Right, Key::Char('t'), Key::Escape].map(Token::Key)
        );
        let a = args(&["--keys", "t,say:name,enter"]).unwrap();
        assert_eq!(
            a.keys,
            [
                Token::Key(Key::Char('t')),
                Token::Say("name".into()),
                Token::Key(Key::Enter)
            ]
        );
        assert!(args(&["--frames"]).is_err());
        assert!(args(&["--bogus"]).is_err());
        assert!(args(&["--keys", "upp"]).is_err());
        assert!(args(&["--seed", "70000"]).is_err());
        assert_eq!(args(&[]).unwrap().frames, 60);
    }

    #[test]
    fn maps_letters_digits_and_arrows() {
        use minifb::Key as K;
        assert_eq!(map_key(K::A), Some(Key::Char('a')));
        assert_eq!(map_key(K::T), Some(Key::Char('t')));
        assert_eq!(map_key(K::Z), Some(Key::Char('z')));
        assert_eq!(map_key(K::Key7), Some(Key::Char('7')));
        assert_eq!(map_key(K::Left), Some(Key::Left));
        assert_eq!(map_key(K::F1), None);
    }

    #[test]
    fn upscale_repeats_each_pixel_in_a_3x3_block() {
        let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];
        rgba[4..8].copy_from_slice(&[1, 2, 3, 255]);
        let mut out = vec![0u32; WIDTH * SCALE * HEIGHT * SCALE];
        upscale(&rgba, &mut out);
        let row = WIDTH * SCALE;
        for (x, y) in [(3, 0), (5, 0), (3, 2), (5, 2)] {
            assert_eq!(out[y * row + x], 0x010203);
        }
        assert_eq!(out[2], 0);
        assert_eq!(out[6], 0);
        assert_eq!(out[3 * row + 3], 0);
    }
}

//! Desktop runner: boots the engine from the user's archive.org zip, then either opens a
//! 3x-scaled window or renders headless to PNG for pixel checks.
//!
//! ```text
//! u6-native --zip ultima-vi.zip [--seed N] [--save-dir DIR] [--game-first]      open the window
//! u6-native --zip ultima-vi.zip --headless-png out.png [--frames N] [--keys up,up,left,t] [--seed N] [--save-dir DIR] [--wav out.wav] [--game-first]
//! ```
//!
//! The engine runs the launcher's sequence: `u` (the opening, the main menu, character
//! creation; Escape skips the opening, `j` journeys onward), then `game`. `--game-first` starts
//! `game` at once, as the oracle's game-first disks do (for comparisons with it); a save without
//! a character then plays the shipped disk state. When the sequence ends (Quit in the menu,
//! the end of the game) the window closes and the runner says so.
//!
//! Sound: the window plays the game's music through the default output device (cpal); headless
//! `--wav PATH` writes the audio of the run (each tick's length at 44.1 kHz, 16-bit stereo) and
//! prints its RMS and peak.
//!
//! `--save-dir DIR` keeps the game's saves in `DIR/savegame/` (the original's PLAYER-disk
//! layout; an original `savegame/` directory copied there loads as it is): the boot reads the
//! saved state from there, and every Save the game makes is written there after its tick.
//! Without it nothing is stored and every boot starts from the zip's new-game files.
//!
//! The window runs the game on the fixed step (spec 06): each window frame runs the 20 ms steps
//! (one PAL frame) the wall time since the previous frame is worth at the speed, while the
//! frame's budget lasts (`pace`). `=` or numpad `+` is faster, `-` or numpad `-` slower,
//! through 1×, 1.5×, 2×, 3×, 4×, 6×, 8× and Max (as many steps as the budget holds); the title
//! shows a speed other than 1×. A mouse button held paces at 1×, so a click walks as at 1×.
//!
//! Headless mode never opens a window or reads a clock: it runs N ticks of `HEADLESS_TICK_MS`
//! (the same fixed step), delivering the `--keys` one per tick while the game has no input
//! queued, and writes the last
//! published frame (320 × 200; built with `--features wide`, spec 13's Wide frame, 912 × 336,
//! shown and written as it is). A `ctrl+<letter>` token is that control character (`ctrl+s` Save,
//! `ctrl+r` Restore; the window sends them for Ctrl + a letter); `tab` is the Tab key (with a
//! cursor up it moves to the inventory panel). A `say:words` token types
//! `words` and Return into the next line
//! input, pressing space (one at a time, after the game has idled a few ticks) for every more
//! prompt or key wait before it. `pointer:X:Y:B` moves the pointer to screen pixel (X, Y) with
//! buttons B held (bit 0 left, bit 1 right); `click:X:Y` (or `click:X:Y:B`) is a press there
//! and its release in the same tick; `wait:N` idles N ticks. `--png-every N:prefix` also writes the frame every
//! N ticks. The RNG seed defaults to 0 headless (reproducible) and to the wall clock in the
//! window (as the original seeds from `time()`). In the window the mouse is the pointer, a key
//! held repeats as repeats and its release takes back those still queued (spec 19 O1b); the
//! `--keys` are presses alone.
//! Headless, `U6_WINDOW_CACHE=off` renders the map window at every refresh, without the cache
//! that leaves out a render that would change nothing (spec 06 O2): the equivalence runs
//! compare the frames of both.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use minifb::{KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use u6_core::pacer::{Pacer, STEP_MS};
use u6_core::saveload::{SaveSync, attach};
use u6_core::{Engine, Files, HEIGHT, InputEvent, Key, WIDTH};
use u6_intake::{InputFile, intake};

mod audio;
mod pace;
mod saves;
use audio::{Audio, Output, WAV_RATE};
use saves::DirStore;

const SCALE: usize = 3;
/// Engine milliseconds of a headless tick: the fixed step, as in the window and the page.
const HEADLESS_TICK_MS: u32 = STEP_MS;

#[derive(Debug, Default, PartialEq)]
struct Args {
    zip: Option<PathBuf>,
    headless_png: Option<PathBuf>,
    frames: u32,
    keys: Vec<Token>,
    seed: Option<u16>,
    save_dir: Option<PathBuf>,
    wav: Option<PathBuf>,
    game_first: bool,
    /// `--png-every N:prefix`: also write the frame every N ticks to `prefix-<tick>.png`.
    png_every: Option<(u32, PathBuf)>,
}

/// One `--keys` token.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Key(Key),
    /// Text typed into the next line input, then Return.
    Say(String),
    /// The pointer at (x, y) with these buttons.
    Pointer(i16, i16, u8),
    /// A press of these buttons at (x, y), then the release.
    Click(i16, i16, u8),
    /// Idle ticks.
    Wait(u32),
}

/// `X:Y` or `X:Y:B` of a pointer token (B defaults to `default`).
fn parse_point(spec: &str, default: u8) -> Result<(i16, i16, u8), String> {
    let bad = || format!("bad pointer {spec:?} (want X:Y or X:Y:B)");
    let mut it = spec.split(':');
    let mut num = |what: &str| -> Result<Option<i64>, String> {
        it.next()
            .map(|v| {
                v.parse::<i64>()
                    .map_err(|_| format!("bad {what} in {spec:?}"))
            })
            .transpose()
    };
    let (Some(x), Some(y)) = (num("x")?, num("y")?) else {
        return Err(bad());
    };
    let b = num("buttons")?.unwrap_or(i64::from(default));
    if it.next().is_some() || !(0..=3).contains(&b) {
        return Err(bad());
    }
    let clamp = |v: i64| v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
    Ok((clamp(x), clamp(y), b as u8))
}

fn parse_token(token: &str) -> Result<Token, String> {
    if let Some(text) = token.strip_prefix("say:") {
        return Ok(Token::Say(text.to_string()));
    }
    if let Some(spec) = token.strip_prefix("pointer:") {
        let (x, y, b) = parse_point(spec, 0)?;
        return Ok(Token::Pointer(x, y, b));
    }
    if let Some(spec) = token.strip_prefix("click:") {
        let (x, y, b) = parse_point(spec, 1)?;
        return Ok(Token::Click(x, y, b));
    }
    if let Some(n) = token.strip_prefix("wait:") {
        return n
            .parse()
            .map(Token::Wait)
            .map_err(|_| format!("bad wait {token:?}"));
    }
    parse_key(token).map(Token::Key)
}

/// The control character of letter `c` (Ctrl-S = 0x13): what the game's key table gives in its
/// control column.
fn control_char(c: char) -> Option<char> {
    c.is_ascii_alphabetic()
        .then(|| char::from(c.to_ascii_lowercase() as u8 & 0x1f))
}

/// One key token: an arrow / control key name, `ctrl+<letter>`, or a single character.
fn parse_key(token: &str) -> Result<Key, String> {
    let lower = token.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("ctrl+") {
        let mut chars = rest.chars();
        return match (chars.next().and_then(control_char), chars.next()) {
            (Some(c), None) => Ok(Key::Char(c)),
            _ => Err(format!("unknown key {token:?}")),
        };
    }
    Ok(match lower.as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "tab" => Key::Char('\t'),
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
            "--wav" => parsed.wav = Some(value("a path")?.into()),
            "--game-first" => parsed.game_first = true,
            "--png-every" => {
                let v = value("N:prefix")?;
                let (n, prefix) = v.split_once(':').ok_or(format!("bad --png-every {v:?}"))?;
                let n: u32 = n.parse().map_err(|_| format!("bad --png-every {v:?}"))?;
                parsed.png_every = Some((n.max(1), prefix.into()));
            }
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
/// files and boots the engine (the launcher's sequence, or `game` at once with `game_first`).
fn boot(zip: &Path, seed: u16, saves: Option<&Saves>, game_first: bool) -> Result<Engine, String> {
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
    let engine = if game_first {
        Engine::game_first(files, seed)
    } else {
        Engine::with_seed(files, seed)
    };
    engine.map_err(|e| e.to_string())
}

/// What the runner prints when the launcher's sequence ended.
const ENDED: &str = "u6-native: the game ended";

fn print_log(engine: &mut Engine) {
    for line in engine.take_log() {
        eprintln!("{line}");
    }
}

/// Ticks the game must idle with nothing queued before a space answers a wait for `say:`.
const IDLE_TICKS: u32 = 3;

/// A pointer event.
fn pointer(x: i16, y: i16, buttons: u8) -> InputEvent {
    InputEvent::Pointer { x, y, buttons }
}

/// Runs `frames` fixed ticks feeding the tokens (see the module doc); returns the last frame as
/// RGBA8. `every` writes the frame every N ticks as well.
fn simulate(
    engine: &mut Engine,
    frames: u32,
    tokens: &[Token],
    mut saves: Option<&mut Saves>,
    mut sound: Option<(&mut Audio, &mut Vec<f32>)>,
    every: Option<&(u32, PathBuf)>,
) -> Result<Vec<u8>, String> {
    let mut queue: std::collections::VecDeque<InputEvent> = std::collections::VecDeque::new();
    let mut next = tokens.iter();
    let mut pending: Option<String> = None;
    let mut idle = 0u32;
    let mut ended = false;
    let mut wait = 0u32;
    let mut rgba = Vec::new();
    for tick in 0..frames {
        let busy = engine.io().pending_events() > 0;
        idle = if busy { 0 } else { idle + 1 };
        let mut input = Vec::new();
        if wait > 0 {
            wait -= 1;
        } else if !busy {
            if let Some(ev) = queue.pop_front() {
                input.push(ev);
            } else if let Some(text) = pending.take() {
                if engine.state().ui.input.line_input {
                    queue.extend(text.chars().map(|c| InputEvent::Key(Key::Char(c))));
                    queue.push_back(InputEvent::Key(Key::Enter));
                    if let Some(ev) = queue.pop_front() {
                        input.push(ev);
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
                    Some(&Token::Pointer(x, y, b)) => input.push(pointer(x, y, b)),
                    Some(&Token::Click(x, y, b)) => {
                        // Both in one tick: the event wait samples the press, then the
                        // release, so the click acts once (a hold would repeat the map walk).
                        input.push(pointer(x, y, b));
                        input.push(pointer(x, y, 0));
                    }
                    Some(&Token::Wait(n)) => wait = n,
                    None => {}
                }
            }
        }
        engine.tick(&input, HEADLESS_TICK_MS);
        if let Some(saves) = saves.as_deref_mut() {
            saves.flush(engine)?;
        }
        if let Some((audio, samples)) = sound.as_mut() {
            audio.sync(engine);
            audio.render_ms(HEADLESS_TICK_MS, samples);
        }
        if engine.finished() && !ended {
            ended = true;
            eprintln!("{ENDED} (tick {})", tick + 1);
        }
        if let Some((n, prefix)) = every
            && (tick + 1) % n == 0
        {
            engine.frame().write_rgba(&mut rgba);
            let path = format!("{}-{:05}.png", prefix.display(), tick + 1);
            write_png(Path::new(&path), &rgba)?;
        }
    }
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

/// A window key; with `ctrl` held a letter is its control character (Ctrl-S Save, Ctrl-R
/// Restore).
fn map_key(key: minifb::Key, ctrl: bool) -> Option<Key> {
    use minifb::Key as K;
    let letter = |k: K| {
        let c = char::from(b'a' + (k as u8 - K::A as u8));
        Key::Char(if ctrl {
            control_char(c).unwrap_or(c)
        } else {
            c
        })
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

/// Spec 19 O1b: per window key held, each game key its presses and repeats were queued as, so
/// its release takes back those whatever the modifiers are then (Ctrl let go before S still
/// releases Ctrl-S).
#[derive(Debug, Default)]
struct HeldKeys(Vec<(minifb::Key, Key)>);

impl HeldKeys {
    /// The event of a press of `key` routed to the game as `k` (`repeat`: minifb's own repeat).
    fn press(&mut self, key: minifb::Key, k: Key, repeat: bool) -> InputEvent {
        if !self.0.contains(&(key, k)) {
            self.0.push((key, k));
        }
        if repeat {
            InputEvent::Repeat(k)
        } else {
            InputEvent::Key(k)
        }
    }

    /// The releases of the game keys window key `key` gave.
    fn release(&mut self, key: minifb::Key) -> Vec<InputEvent> {
        let mut out = Vec::new();
        self.0.retain(|&(held, k)| {
            if held != key {
                return true;
            }
            out.push(InputEvent::Release(k));
            false
        });
        out
    }

    /// Spec 19 O1c: the newest direction key held, which the game presses again at the command
    /// prompt ([`Engine::held_press`]).
    fn direction(&self) -> Option<Key> {
        self.0
            .iter()
            .rev()
            .map(|&(_, k)| k)
            .find(|k| k.is_direction())
    }

    /// The releases of every key held (the window lost the focus: their releases may not come).
    fn release_all(&mut self) -> Vec<InputEvent> {
        self.0
            .drain(..)
            .map(|(_, k)| InputEvent::Release(k))
            .collect()
    }
}

/// What a window key does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Route {
    /// A key for the game.
    Game(Key),
    /// The next speed up ([`pace::faster`]).
    Faster,
    /// The next speed down ([`pace::slower`]).
    Slower,
}

/// A window key's route (spec 06 §Design 6): `=` and numpad `+` faster, `-` and numpad `-`
/// slower, whatever the game is doing; every other key as [`map_key`] maps it. [`map_key`]
/// gives the game none of the four (nor a shifted character such as `+`), so no key the game
/// takes changes hands, and unlike the page's there is no owner check
/// ([`u6_core::Engine::keys_owned`]).
fn route(key: minifb::Key, ctrl: bool) -> Option<Route> {
    use minifb::Key as K;
    match key {
        K::Equal | K::NumPadPlus => Some(Route::Faster),
        K::Minus | K::NumPadMinus => Some(Route::Slower),
        _ => map_key(key, ctrl).map(Route::Game),
    }
}

/// One step of the window: the engine ticks [`STEP_MS`] with the input not yet delivered (a
/// frame's input goes to its first step, as the page queues keys for its next step; a frame
/// that runs none keeps it for the next), and the direction key `held` down pressed again when
/// the command prompt is up (spec 19 O1c, [`Engine::held_press`]); the store writes what a Save
/// in the step committed, and the sound machine replays the step's calls (after every step:
/// the call log holds one step's). False once the game ended.
fn window_step(
    engine: &mut Engine,
    pending: &mut Vec<InputEvent>,
    held: Option<Key>,
    saves: Option<&mut Saves>,
    audio: Option<&mut Audio>,
) -> Result<bool, String> {
    if let Some(press) = engine.held_press(pending, held) {
        pending.push(press);
    }
    engine.tick(pending, STEP_MS);
    pending.clear();
    if let Some(saves) = saves {
        saves.flush(engine)?;
    }
    if let Some(audio) = audio {
        audio.sync(engine);
    }
    Ok(!engine.finished())
}

/// The mouse this frame as the game's pointer: `pos` in window pixels scaled to screen pixels,
/// and the `left` / `right` buttons ([`u6_core::engine::POINTER_LEFT`], `POINTER_RIGHT`). The
/// buttons are read every frame on their own: without a position (minifb 0.29 always has one
/// in [`MouseMode::Clamp`], the edge of the window when the mouse is outside it; this keeps a
/// backend that had none from losing a release) they come at the `last` position, so a
/// release always reaches the game and a held button cannot stick. None while there was never
/// a position.
fn sample_mouse(
    pos: Option<(f32, f32)>,
    left: bool,
    right: bool,
    last: Option<(i16, i16, u8)>,
) -> Option<(i16, i16, u8)> {
    let buttons = u8::from(left) | u8::from(right) << 1;
    let (x, y) = match pos {
        Some((x, y)) => ((x as usize / SCALE) as i16, (y as usize / SCALE) as i16),
        None => last.map(|(x, y, _)| (x, y))?,
    };
    Some((x, y, buttons))
}

fn run_window(mut engine: Engine, mut saves: Option<Saves>) -> Result<(), String> {
    let (w, h) = (WIDTH * SCALE, HEIGHT * SCALE);
    let mut sound = match Output::open() {
        Ok(out) => Audio::or_silent(&engine, out.rate).map(|audio| (audio, out)),
        Err(e) => {
            eprintln!("audio: {e}; running silent");
            None
        }
    };
    let mut pacer = Pacer::new();
    let mut window = Window::new(&pace::title(pacer.speed()), w, h, WindowOptions::default())
        .map_err(|e| e.to_string())?;
    window.set_target_fps(60);
    let mut rgba = Vec::new();
    let mut screen = vec![0u32; w * h];
    // The engine frame `screen` shows (`Engine::frame_version`; none yet: the window opens black).
    let mut shown = None;
    let mut last = Instant::now();
    let mut mouse = None;
    let mut pending: Vec<InputEvent> = Vec::new();
    let mut keys = HeldKeys::default();
    while window.is_open() {
        let ctrl =
            window.is_key_down(minifb::Key::LeftCtrl) || window.is_key_down(minifb::Key::RightCtrl);
        let mut speed = pacer.speed();
        for key in window.get_keys_pressed(KeyRepeat::Yes) {
            // Spec 19 O1b: a press minifb's own repeat sent (the key held past its delay) goes
            // as a repeat, which the key's release takes back.
            let repeat = !window.is_key_pressed(key, KeyRepeat::No);
            match route(key, ctrl) {
                Some(Route::Game(k)) => pending.push(keys.press(key, k, repeat)),
                Some(Route::Faster) => speed = pace::faster(speed),
                Some(Route::Slower) => speed = pace::slower(speed),
                None => {}
            }
        }
        for key in window.get_keys_released() {
            pending.extend(keys.release(key));
        }
        if !window.is_active() {
            pending.extend(keys.release_all());
        }
        if speed != pacer.speed() {
            pacer.set_speed(speed);
            window.set_title(&pace::title(speed));
            if let Some((_, out)) = sound.as_mut() {
                out.set_speed(speed);
            }
        }
        // The mouse, once per frame when it changed.
        let now = sample_mouse(
            window.get_mouse_pos(MouseMode::Clamp),
            window.get_mouse_down(MouseButton::Left),
            window.get_mouse_down(MouseButton::Right),
            mouse,
        );
        if now != mouse {
            if let Some((x, y, buttons)) = now {
                pending.push(pointer(x, y, buttons));
            }
            mouse = now;
        }
        // The handler's buttons after this frame's input: the last pointer state queued (still
        // pending or taken).
        let held = mouse.is_some_and(|(_, _, buttons)| buttons != 0);
        // Spec 19 O1c: the direction key down, unless a modifier is (Ctrl, Alt or the system
        // key with an arrow is no direction key).
        let modifier = {
            use minifb::Key as K;
            [
                K::LeftCtrl,
                K::RightCtrl,
                K::LeftAlt,
                K::RightAlt,
                K::LeftSuper,
                K::RightSuper,
            ]
            .into_iter()
            .any(|k| window.is_key_down(k))
        };
        let direction = keys.direction().filter(|_| !modifier);
        let frame_at = Instant::now();
        let wall_dt_ms = frame_at.duration_since(last).as_secs_f64() * 1000.0;
        last = frame_at;
        let steps = pace::run_frame(
            &mut pacer,
            wall_dt_ms,
            held,
            || frame_at.elapsed().as_secs_f64() * 1000.0,
            || {
                window_step(
                    &mut engine,
                    &mut pending,
                    direction,
                    saves.as_mut(),
                    sound.as_mut().map(|(audio, _)| audio),
                )
            },
        )?;
        if let Some((audio, out)) = sound.as_mut() {
            out.top_up(audio);
        }
        print_log(&mut engine);
        // A frame whose steps composed no new picture, or that ran none, shows the picture it
        // had (spec 06 O1).
        if steps > 0 && shown != Some(engine.frame_version()) {
            engine.frame().write_rgba(&mut rgba);
            upscale(&rgba, &mut screen);
            shown = Some(engine.frame_version());
        }
        window
            .update_with_buffer(&screen, w, h)
            .map_err(|e| e.to_string())?;
        if engine.finished() {
            eprintln!("{ENDED}");
            break;
        }
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
            let mut engine = boot(zip, args.seed.unwrap_or(0), saves.as_ref(), args.game_first)?;
            if std::env::var_os("U6_WINDOW_CACHE").is_some_and(|v| v == "off") {
                engine.io().set_window_cache(false);
            }
            let mut audio = match &args.wav {
                Some(_) => Some(Audio::new(&engine, WAV_RATE)?),
                None => None,
            };
            let mut samples = Vec::new();
            let sound = audio.as_mut().map(|a| (a, &mut samples));
            let rgba = simulate(
                &mut engine,
                args.frames,
                &args.keys,
                saves.as_mut(),
                sound,
                args.png_every.as_ref(),
            )?;
            print_log(&mut engine);
            write_png(path, &rgba)?;
            if let Some(wav) = &args.wav {
                audio::write_wav(wav, &samples, WAV_RATE)?;
                let (rms, peak) = audio::levels(&samples);
                let s = engine.state();
                println!(
                    "wrote {} ({:.2} s, rms {rms:.4}, peak {peak:.4}; sound {}, song {} {})",
                    wav.display(),
                    samples.len() as f64 / 2.0 / f64::from(WAV_RATE),
                    if u6_core::sound::sound_on(&s) {
                        "on"
                    } else {
                        "off"
                    },
                    s.sound.current,
                    s.sound.file_name(s.sound.current).unwrap_or("-"),
                );
            }
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
            let seed = args.seed.unwrap_or_else(clock_seed);
            let engine = boot(zip, seed, saves.as_ref(), args.game_first)?;
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

    /// Spec 19 O1b: Ctrl-S held into its repeats, Ctrl let go first (S's next repeat routes as
    /// a plain `s`), then S: both of S's game keys are released, the arrow's not.
    #[test]
    fn a_release_goes_by_the_window_key_whatever_the_modifiers() {
        use minifb::Key as K;
        let mut held = HeldKeys::default();
        let (ctrl_s, s) = (Key::Char('\u{13}'), Key::Char('s'));
        let route_game = |key, ctrl| match route(key, ctrl) {
            Some(Route::Game(k)) => k,
            other => panic!("{other:?}"),
        };
        assert_eq!(route_game(K::S, true), ctrl_s);
        let presses = [
            held.press(K::S, route_game(K::S, true), false),
            held.press(K::S, route_game(K::S, true), true),
            held.press(K::S, route_game(K::S, false), true),
            held.press(K::Up, route_game(K::Up, false), false),
        ];
        assert_eq!(
            presses,
            [
                InputEvent::Key(ctrl_s),
                InputEvent::Repeat(ctrl_s),
                InputEvent::Repeat(s),
                InputEvent::Key(Key::Up)
            ]
        );
        assert_eq!(
            held.release(K::S),
            [InputEvent::Release(ctrl_s), InputEvent::Release(s)]
        );
        assert!(held.release(K::S).is_empty(), "released once");
        assert_eq!(held.release_all(), [InputEvent::Release(Key::Up)]);
        assert!(held.release_all().is_empty());
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
            args(&["--wav", "boot.wav"]).unwrap().wav,
            Some(PathBuf::from("boot.wav"))
        );
        assert_eq!(a.wav, None);
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
        assert_eq!(
            args(&["--keys", "ctrl+s,CTRL+R"]).unwrap().keys,
            [Key::Char('\u{13}'), Key::Char('\u{12}')].map(Token::Key)
        );
        assert!(args(&["--keys", "ctrl+1"]).is_err());
        assert_eq!(
            args(&["--keys", "tab"]).unwrap().keys,
            [Token::Key(Key::Char('\t'))]
        );
        assert!(args(&["--seed", "70000"]).is_err());
        assert_eq!(args(&[]).unwrap().frames, 60);
        assert!(!args(&[]).unwrap().game_first, "the launcher by default");
        assert!(args(&["--game-first"]).unwrap().game_first);
    }

    #[test]
    fn parses_pointer_click_and_wait_tokens() {
        let a = args(&[
            "--keys",
            "pointer:24:184:0,click:100:90,click:200:40:2,wait:5",
        ])
        .unwrap();
        assert_eq!(
            a.keys,
            [
                Token::Pointer(24, 184, 0),
                Token::Click(100, 90, 1),
                Token::Click(200, 40, 2),
                Token::Wait(5)
            ]
        );
        assert!(args(&["--keys", "click:1"]).is_err());
        assert!(args(&["--keys", "pointer:1:2:4"]).is_err());
        assert!(args(&["--keys", "click:1:2:1:9"]).is_err());
        assert!(args(&["--keys", "wait:x"]).is_err());
        let a = args(&["--png-every", "10:/tmp/f"]).unwrap();
        assert_eq!(a.png_every, Some((10, PathBuf::from("/tmp/f"))));
    }

    #[test]
    fn maps_letters_digits_and_arrows() {
        use minifb::Key as K;
        assert_eq!(map_key(K::A, false), Some(Key::Char('a')));
        assert_eq!(map_key(K::T, false), Some(Key::Char('t')));
        assert_eq!(map_key(K::Z, false), Some(Key::Char('z')));
        assert_eq!(map_key(K::Key7, false), Some(Key::Char('7')));
        assert_eq!(map_key(K::Left, false), Some(Key::Left));
        assert_eq!(map_key(K::F1, false), None);
        assert_eq!(map_key(K::S, true), Some(Key::Char('\u{13}')));
        assert_eq!(map_key(K::Left, true), Some(Key::Left));
    }

    /// The speed keys against the game's: the four speed keys whatever the modifiers, every
    /// other key as [`map_key`] has it; and [`map_key`] gives the game none of the four (so the
    /// game's state, [`u6_core::Engine::keys_owned`], has nothing to claim).
    #[test]
    fn routes_the_speed_keys_and_leaves_the_rest_to_the_game() {
        use minifb::Key as K;
        let speed = [
            (K::Equal, Route::Faster),
            (K::NumPadPlus, Route::Faster),
            (K::Minus, Route::Slower),
            (K::NumPadMinus, Route::Slower),
        ];
        for ctrl in [false, true] {
            for (key, want) in speed {
                assert_eq!(route(key, ctrl), Some(want), "{key:?} ctrl {ctrl}");
                assert_eq!(map_key(key, ctrl), None, "{key:?} never reached the game");
            }
            for key in [
                K::A,
                K::S,
                K::Key0,
                K::Key9,
                K::Up,
                K::Enter,
                K::NumPadEnter,
                K::Escape,
                K::Space,
                K::Backspace,
                K::F1,
                K::LeftShift,
                K::NumPad5,
                K::NumPadAsterisk,
            ] {
                assert_eq!(
                    route(key, ctrl),
                    map_key(key, ctrl).map(Route::Game),
                    "{key:?} ctrl {ctrl}"
                );
            }
        }
        assert_eq!(route(K::S, true), Some(Route::Game(Key::Char('\u{13}'))));
        assert_eq!(route(K::F1, false), None);
    }

    /// Spec 19 O1c: the window step presses the held direction key again whenever the command
    /// prompt is up (a game that reads one key with A4-0x7a2c set, then waits 100 ms): at every
    /// read, with no key from the window after the first; a held letter, none.
    #[test]
    fn the_window_step_presses_a_held_direction_key_at_the_prompt() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let taken = |held: Option<Key>| {
            let seen = Rc::new(RefCell::new(0));
            let out = Rc::clone(&seen);
            let mut engine = Engine::with_game(
                u6_core::State::default(),
                Files::default(),
                move |st, io| async move {
                    loop {
                        st.borrow_mut().main.reading = 1;
                        io.next_key().await;
                        st.borrow_mut().main.reading = 0;
                        *out.borrow_mut() += 1;
                        io.sleep(100).await;
                    }
                },
            );
            let mut pending = Vec::new();
            for _ in 0..60 {
                window_step(&mut engine, &mut pending, held, None, None).unwrap();
            }
            *seen.borrow()
        };
        assert!(taken(Some(Key::Left)) >= 8, "at every read");
        assert_eq!(
            taken(Some(Key::Char('x'))),
            0,
            "a letter is not pressed again"
        );
        assert_eq!(taken(None), 0);
    }

    /// A frame's input goes to its first step only, and a frame that runs no step keeps it
    /// for the next: a game that takes every event as it comes records the step (the tick
    /// count, from 1) each one reached it in.
    #[test]
    fn the_frames_input_goes_to_its_first_step() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let seen: Rc<RefCell<Vec<(u64, InputEvent)>>> = Rc::default();
        let log = Rc::clone(&seen);
        let mut engine = Engine::with_game(
            u6_core::State::default(),
            Files::default(),
            |_, io| async move {
                loop {
                    let step = io.generation() + 1;
                    while io.pending_events() > 0 {
                        let ev = if io.key_ready() {
                            InputEvent::Key(io.next_key().await)
                        } else {
                            let p = io.take_pointer();
                            pointer(p.x, p.y, p.buttons)
                        };
                        log.borrow_mut().push((step, ev));
                    }
                    io.vblank().await;
                }
            },
        );
        let mut pacer = Pacer::new();
        let mut pending = Vec::new();
        let mut frame = |engine: &mut Engine, input: &[InputEvent], dt: f64| {
            pending.extend_from_slice(input);
            let k = pace::run_frame(
                &mut pacer,
                dt,
                false,
                || 0.0,
                || window_step(engine, &mut pending, None, None, None),
            )
            .unwrap();
            (k, pending.len())
        };
        let (a, at) = (InputEvent::Key(Key::Char('a')), pointer(1, 2, 0));
        let (b, c) = (
            InputEvent::Key(Key::Char('b')),
            InputEvent::Key(Key::Char('c')),
        );
        assert_eq!(
            frame(&mut engine, &[a, at], 10.0),
            (0, 2),
            "10 ms: no step, kept"
        );
        assert_eq!(frame(&mut engine, &[], 100.0), (5, 0), "110 ms: steps 1-5");
        assert_eq!(frame(&mut engine, &[b], 30.0), (2, 0), "140 ms: steps 6-7");
        assert_eq!(
            frame(&mut engine, &[c], 5.0),
            (0, 1),
            "145 ms: no step, kept"
        );
        assert_eq!(frame(&mut engine, &[], 15.0), (1, 0), "160 ms: step 8");
        assert_eq!(*seen.borrow(), [(1, a), (1, at), (6, b), (8, c)]);
        assert_eq!((engine.ticks(), engine.io().now_ms()), (8, 160));
    }

    #[test]
    fn the_mouse_buttons_are_read_with_or_without_a_position() {
        let s = SCALE as f32;
        let press = sample_mouse(Some((10.0 * s, 20.0 * s + 2.0)), true, false, None);
        assert_eq!(press, Some((10, 20, 1)), "window pixels to screen pixels");
        let right = sample_mouse(Some((10.0 * s, 20.0 * s)), true, true, press);
        assert_eq!(right, Some((10, 20, 3)));
        // No position: the buttons as they are, where the pointer was last.
        assert_eq!(sample_mouse(None, true, true, right), right, "still held");
        let release = sample_mouse(None, false, false, right);
        assert_eq!(release, Some((10, 20, 0)), "the release reaches the game");
        assert_eq!(
            sample_mouse(None, true, false, None),
            None,
            "never a position"
        );
        assert_eq!(
            sample_mouse(Some((0.0, 0.0)), false, false, release),
            Some((0, 0, 0))
        );
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

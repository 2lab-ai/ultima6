//! Desktop runner: a 3x-scaled window for development, or a deterministic headless
//! render to PNG for pixel checks.
//!
//! ```text
//! u6-native                                   open the window
//! u6-native --headless-png out.png [--frames N] [--hold-right|--hold-left|--hold-up|--hold-down]
//! ```
//!
//! Headless mode never opens a window or reads a clock: it runs N ticks of
//! `HEADLESS_TICK_MS` with the given keys held and writes the 320x200 RGBA frame.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use minifb::{Window, WindowOptions};
use u6_core::{Framebuffer, Game, HEIGHT, Input, Key, WIDTH};

const SCALE: usize = 3;
const HEADLESS_TICK_MS: u32 = 16;
const MAX_STEP_MS: u64 = 100;

#[derive(Debug, Default, PartialEq)]
struct Args {
    headless_png: Option<PathBuf>,
    frames: u32,
    hold: Vec<Key>,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        frames: 60,
        ..Args::default()
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless-png" => {
                let path = args.next().ok_or("--headless-png needs a path")?;
                parsed.headless_png = Some(path.into());
            }
            "--frames" => {
                let n = args.next().ok_or("--frames needs a number")?;
                parsed.frames = n.parse().map_err(|_| format!("bad --frames {n:?}"))?;
            }
            "--hold-right" => parsed.hold.push(Key::Right),
            "--hold-left" => parsed.hold.push(Key::Left),
            "--hold-up" => parsed.hold.push(Key::Up),
            "--hold-down" => parsed.hold.push(Key::Down),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(parsed)
}

fn map_key(key: minifb::Key) -> Option<Key> {
    use minifb::Key as K;
    Some(match key {
        K::Up => Key::Up,
        K::Down => Key::Down,
        K::Left => Key::Left,
        K::Right => Key::Right,
        K::Enter | K::NumPadEnter => Key::Enter,
        K::Escape => Key::Escape,
        K::Space => Key::Space,
        _ => return None,
    })
}

/// Runs `frames` fixed ticks with `hold` pressed and returns the final frame as RGBA8.
fn simulate(frames: u32, hold: &[Key]) -> Vec<u8> {
    let mut game = Game::new();
    let mut input = Input::default();
    for &key in hold {
        input.set(key, true);
    }
    for _ in 0..frames {
        game.tick(&input, HEADLESS_TICK_MS);
    }
    let mut fb = Framebuffer::new();
    game.render(&mut fb);
    let mut rgba = Vec::new();
    fb.write_rgba(&mut rgba);
    rgba
}

fn write_png(path: &PathBuf, rgba: &[u8]) -> Result<(), String> {
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

fn run_window() -> Result<(), String> {
    let (w, h) = (WIDTH * SCALE, HEIGHT * SCALE);
    let mut window =
        Window::new("Ultima VI", w, h, WindowOptions::default()).map_err(|e| e.to_string())?;
    window.set_target_fps(60);
    let mut game = Game::new();
    let mut input = Input::default();
    let mut fb = Framebuffer::new();
    let mut rgba = Vec::new();
    let mut screen = vec![0u32; w * h];
    let start = Instant::now();
    let mut delivered_ms = 0u64;
    while window.is_open() {
        input.clear();
        for key in window.get_keys().into_iter().filter_map(map_key) {
            input.set(key, true);
        }
        let now_ms = start.elapsed().as_millis() as u64;
        let step = (now_ms - delivered_ms).min(MAX_STEP_MS);
        delivered_ms = now_ms;
        game.tick(&input, step as u32);
        game.render(&mut fb);
        fb.write_rgba(&mut rgba);
        upscale(&rgba, &mut screen);
        window
            .update_with_buffer(&screen, w, h)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let result = parse_args(std::env::args().skip(1)).and_then(|args| match &args.headless_png {
        Some(path) => {
            write_png(path, &simulate(args.frames, &args.hold))?;
            println!("wrote {} ({} frames)", path.display(), args.frames);
            Ok(())
        }
        None => run_window(),
    });
    match result {
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
    fn parses_headless_flags() {
        let a = args(&["--headless-png", "o.png", "--frames", "5", "--hold-right"]).unwrap();
        assert_eq!(a.headless_png, Some(PathBuf::from("o.png")));
        assert_eq!((a.frames, a.hold), (5, vec![Key::Right]));
        assert!(args(&["--frames"]).is_err());
        assert!(args(&["--bogus"]).is_err());
    }

    #[test]
    fn headless_hold_right_puts_marker_at_209() {
        let rgba = simulate(60, &[Key::Right]);
        let at = |x: usize, y: usize| &rgba[(y * WIDTH + x) * 4..][..4];
        assert_eq!(at(209, 92), [255, 255, 255, 255]);
        assert_eq!(at(224, 107), [255, 255, 255, 255]);
        assert_eq!(at(208, 92), [0, 0, 0, 255]);
        assert_eq!(at(225, 92), [0, 0, 0, 255]);
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

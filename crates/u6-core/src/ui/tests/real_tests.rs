//! Real data: the user's zip → u6-intake → `State` + `UiState::load`, the start state's panels
//! and message scroll rendered headless to /tmp/claude-501/p4-ui*.png. Skips without the zip;
//! the data is read only, never copied, and the test prints numbers and the drawn rows' lengths
//! only (the text is the user's).
#![allow(clippy::disallowed_methods)] // the test reads the zip and writes the PNG receipts

use super::*;
use crate::image::GameImage;
use crate::ui::text::WindowId;
use u6_intake::{InputFile, intake};

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

pub(super) fn files() -> Option<Files> {
    if !std::path::Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

fn start_state(files: &Files) -> State {
    let mut s = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
    s.ui = UiState::load(files).unwrap_or_else(|e| panic!("{e}"));
    let t = s.ui.tables.clone();
    s.ui.text.main_windows(&t);
    s
}

/// Draws the panel for `mode` / `member` and the command prompt, like one command-loop
/// iteration up to its key wait (0x1041d8: prompt, 0x103f64), then presents.
fn render(files: &Files, mode: u16, member: i16) -> Engine {
    let mut s = start_state(files);
    s.ui.panel.mode = mode;
    s.ui.panel.member = member;
    s.ui.panel.dirty = true;
    let prompt = GameImage::parse(files.get("game").unwrap())
        .unwrap()
        .cstr(PROMPT)
        .unwrap();
    run(s, &[], 4, move |st, io| async move {
        debug_render(&st, &io, &prompt).await;
    })
}

/// The drawn text of window `id`, row by row (cells in the window's ink, the alert ink, or the
/// poisoned ink).
fn window_text(e: &Engine, id: WindowId) -> Vec<String> {
    let s = e.state();
    let w = *s.ui.text.window(id);
    let c = s.ui.tables.colours;
    let fb = e.frame();
    (0..usize::from(w.height))
        .map(|r| {
            let row = usize::from(w.y) + r;
            (0..usize::from(w.width))
                .map(|k| {
                    let col = usize::from(w.x) + k;
                    let b = [c.text, c.alert, c.poisoned]
                        .iter()
                        .map(|&ink| cell_glyph(fb, &s, col, row, ink))
                        .find(|&b| b != 0)
                        .unwrap_or(0);
                    if (0x20..0x7f).contains(&b) {
                        char::from(b)
                    } else {
                        ' '
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// The glyph code drawn at a cell in `ink` over the paper colour, matched against the font.
fn cell_glyph(fb: &Framebuffer, s: &State, col: usize, row: usize, ink: u8) -> u8 {
    let Some(font) = s.ui.font.as_ref() else {
        return 0;
    };
    let paper = s.ui.tables.colours.paper;
    let mut rows = [0u8; 8];
    for (dy, r) in rows.iter_mut().enumerate() {
        for dx in 0..8 {
            match fb.get(col * 8 + dx, row * 8 + dy) {
                Some(p) if p == ink => *r |= 0x80 >> dx,
                Some(p) if p == paper => {}
                _ => return 0,
            }
        }
    }
    font.glyphs
        .iter()
        .position(|g| *g == rows)
        .map_or(0, |c| c as u8)
}

/// A PNG of the frame (RGB, zlib stored blocks).
pub(super) fn png(fb: &Framebuffer) -> Vec<u8> {
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
    let (w, h) = (fb.width(), fb.height());
    let mut rgba = Vec::new();
    fb.write_rgba(&mut rgba);
    let mut raw = Vec::with_capacity(h * (w * 3 + 1));
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&rgba[(y * w + x) * 4..(y * w + x) * 4 + 3]);
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
    out
}

#[test]
fn real_tables_font_and_start_state_render() {
    let Some(files) = files() else { return };
    let s = start_state(&files);
    let t = &s.ui.tables;
    // Spot checks of the extracted table against what the code implies: raw 0x20 is `a`,
    // the cursor keys give the PC codes 0x148 / 0x150 / 0x14d / 0x14b, return 0x0d.
    assert_eq!(t.keymap[0x20][..2], [0x61, 0x41]);
    assert_eq!(
        [0x4c, 0x4d, 0x4e, 0x4f].map(|r| t.keymap[r][0]),
        [0x148, 0x150, 0x14d, 0x14b]
    );
    assert_eq!(t.keymap[0x44][0], 0x0d);
    // Ctrl-S and Ctrl-R (Save, Restore) come out of the table's control column.
    let mut q = crate::ui::input::Qualifiers::default();
    for c in ['\u{13}', '\u{12}'] {
        let code = crate::ui::input::key_code(&mut q, &t.keymap, crate::Key::Char(c));
        assert_eq!(code, c as u16, "{c:?}");
    }
    assert_eq!(t.panel.combat_modes.len(), 7);
    assert!(s.ui.font.as_ref().is_some_and(|f| f.glyphs.len() == 256));
    println!(
        "colours text {} paper {} alert {}; panel mode {:#x}; window flags {:#x}",
        t.colours.text,
        t.colours.paper,
        t.colours.alert,
        t.initial.panel_mode,
        t.initial.window_flags
    );
    for (mode, file) in [(0x91, "p4-ui.png"), (0x90, "p4-ui-stats.png")] {
        let e = render(&files, mode, 0);
        let path = format!("/tmp/claude-501/{file}");
        std::fs::write(&path, png(e.frame())).unwrap();
        println!("{path}: panel mode {mode:#x}");
        for id in [WindowId::Status, WindowId::Messages] {
            for (r, line) in window_text(&e, id).iter().enumerate() {
                if !line.is_empty() {
                    println!("  {id:?} row {r}: {} characters", line.chars().count());
                }
            }
        }
        let st = e.state();
        assert!(!st.ui.panel.dirty);
        let stubs: Vec<_> = e.stub_hits().into_keys().collect();
        println!("  stubs hit: {stubs:?}");
    }
}

//! Spec 23's independent baseline: with the message history never scrolled back, the shown, the
//! published and the HD frames are the frames of the tree without the feature. The digests below
//! were recorded by this same file at 60f92c6 (the parent commit, before spec 23 existed), so
//! the oracle shares none of the feature's code. A scripted run prints and scrolls the message
//! window over a paper with a speck in it, then takes presses on the bar's column and inside the
//! window, keys, a repeat, a release and text (each one the game prints a line for) and lines on
//! a timer; no wheel. Digests only (FNV-1a 64), no pixels.

use u6_render::hd::Scale;
use u6_render::{BITMAP_PAPER, Framebuffer, Src};

use super::*;
use crate::engine::{POINTER_LEFT, POINTER_RIGHT, Typed};
use crate::hd::HdView;
use crate::main_loop::Program;
use crate::ui::text::Paper;

/// The digests of the parent's run ([`digests`]): the shown frame, the published frame, the HD
/// frames at 2× and 4×, each over every tick, and the ticks.
#[cfg(not(feature = "wide"))]
const PARENT: [u64; 5] = [
    0x2952_cebe_a714_8e55,
    0x2952_cebe_a714_8e55,
    0xb714_d7a8_bdc0_7565,
    0x115b_5165_7f55_62d1,
    154,
];
#[cfg(feature = "wide")]
const PARENT: [u64; 5] = [
    0x97ae_862b_3a94_f655,
    0x97ae_862b_3a94_f655,
    0x37ac_142c_3bb8_ea65,
    0x46d9_f2de_47b7_7ad1,
    154,
];

/// A paper all [`PAPER`] but one pixel of colour 7 inside the window's last row (x 200, y 190).
fn speck() -> u6_formats::bitmap::Bitmap {
    let mut pixels = vec![PAPER; 320 * 200];
    pixels[190 * 320 + 200] = 7;
    u6_formats::bitmap::Bitmap {
        width: 320,
        height: 200,
        pixels,
    }
}

/// FNV-1a 64 of each word into `h`.
fn mix(h: &mut u64, words: impl IntoIterator<Item = u64>) {
    for w in words {
        *h ^= w;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// A frame's colours and its sources, each pixel two words.
fn frame_words(fb: &Framebuffer) -> impl Iterator<Item = u64> + '_ {
    let src = fb.src();
    fb.pixels().iter().enumerate().flat_map(move |(i, &c)| {
        let s = src.map_or(Src::CLASSIC, |p| p[i]);
        let a = (u64::from(c) << 32) | (u64::from(s.kind as u8) << 16) | u64::from(s.flags);
        let b = (u64::from(s.ver) << 32) | (u64::from(s.u) << 16) | u64::from(s.v);
        [a, (u64::from(s.id) << 48) ^ b]
    })
}

/// The input of tick `t` after the game's lines (none past the script).
fn script(t: usize) -> Vec<InputEvent> {
    let p = |x: i16, y: i16, buttons: u8| InputEvent::Pointer { x, y, buttons };
    let a = Key::Char('a');
    match t {
        0 => vec![p(40, 40, 0)],
        // On the bar's column (x 304..312 of the window's last cell), down, a drag, up.
        1 => vec![p(310, 120, POINTER_LEFT)],
        2 => vec![p(310, 140, POINTER_LEFT)],
        3 => vec![p(310, 140, 0)],
        // Inside the window, right.
        4 => vec![p(200, 150, POINTER_RIGHT)],
        5 => vec![p(210, 150, POINTER_RIGHT), p(210, 150, 0)],
        6 => vec![InputEvent::Key(a)],
        7 => vec![InputEvent::Repeat(a), InputEvent::Release(a)],
        8 => vec![InputEvent::Text('z')],
        // On the map: left, both, left, up; then right on the bar's column with left held.
        9 => vec![p(40, 40, POINTER_LEFT)],
        10 => vec![p(42, 40, POINTER_LEFT | POINTER_RIGHT)],
        11 => vec![p(44, 40, POINTER_LEFT)],
        12 => vec![p(310, 130, POINTER_LEFT)],
        13 => vec![p(310, 130, POINTER_LEFT | POINTER_RIGHT)],
        14 => vec![p(310, 130, 0)],
        15 => vec![InputEvent::Key(Key::Enter)],
        _ => Vec::new(),
    }
}

/// The scripted run's digests ([`PARENT`]'s order).
fn digests() -> [u64; 5] {
    let mut s = state();
    s.main.program = Program::Game;
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    s.ui.paper = Some(Paper::drawn(&identity, &speck()));
    let lines = 30u16;
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        io.draw(|fb| {
            for (i, c) in fb.palette.iter_mut().enumerate() {
                let v = i as u8;
                *c = u6_render::Rgb::new(v.wrapping_mul(9), 255 - v, 7);
            }
            crate::gfx::blit::blit_bmp(&identity, fb, &speck(), 0, 0, BITMAP_PAPER);
        });
        printf(&st, &io, b"say @name. ok\n", &[]).await;
        for i in 0..lines {
            printf(&st, &io, b"l%d\n", &[Arg::Word(i)]).await;
            io.present();
            io.vblank().await;
        }
        // Every event the game takes prints a line: a press or a release the buttons, a key or
        // text its count; every 25th frame without one a line on the timer.
        let (mut held, mut n, mut frames) = (0u8, 0u16, 0u16);
        loop {
            let mut printed = false;
            while io.pending_events() > 0 {
                if io.typed_ready() {
                    let w = match io.next_typed().await {
                        Typed::Key(_) => 1,
                        Typed::Text(_) => 2,
                    };
                    printf(&st, &io, b"k%d %d\n", &[Arg::Word(w), Arg::Word(n)]).await;
                } else {
                    let p = io.take_pointer();
                    if p.buttons == held {
                        continue;
                    }
                    held = p.buttons;
                    let b = u16::from(held);
                    printf(&st, &io, b"b%d %d\n", &[Arg::Word(b), Arg::Word(n)]).await;
                }
                n += 1;
                printed = true;
            }
            frames += 1;
            if !printed && frames.is_multiple_of(25) {
                printf(&st, &io, b"t%d\n", &[Arg::Word(frames)]).await;
                printed = true;
            }
            if printed {
                io.present();
            }
            io.vblank().await;
        }
    });
    e.enable_src_plane();
    let blank = Framebuffer::new();
    let mut hd: Vec<HdView> = [Scale::X2, Scale::X4]
        .into_iter()
        .map(|scale| {
            let mut v = HdView::new();
            v.set(true, scale);
            v
        })
        .collect();
    let mut out = [0xcbf2_9ce4_8422_2325u64; 5];
    let ticks = usize::from(lines) + 4 + 120;
    for t in 0..ticks {
        let input = t
            .checked_sub(usize::from(lines) + 4)
            .map_or_else(Vec::new, script);
        e.tick(&input, 16);
        mix(&mut out[0], frame_words(e.frame()));
        mix(&mut out[1], frame_words(&e.published_frame()));
        for (k, v) in hd.iter_mut().enumerate() {
            let wrote = v.present(Some(&e), &blank);
            mix(&mut out[2 + k], [u64::from(wrote)]);
            if wrote {
                mix(&mut out[2 + k], v.rgba().iter().map(|&w| u64::from(w)));
            }
        }
    }
    out[4] = ticks as u64;
    out
}

#[test]
fn with_no_scroll_the_frames_are_the_parents() {
    let got = digests();
    println!("{got:#018x?}");
    assert_eq!(got, PARENT, "shown, published, HD 2×, HD 4×, ticks");
}

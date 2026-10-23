//! Spec 13 C3, in both builds: the view's cells land at `MAP_X`/`MAP_Y` + cell · 16 with the
//! border on the view's first and last cells, the screen shows exactly [`SHOWN`], the map's
//! dissolve reveals [`SHOWN`] once in the original's steps, and a write from the UI stays on the
//! UI's screen (Design 4). Synthetic tiles, no game data.

use std::collections::BTreeSet;

use u6_render::{Framebuffer, HEIGHT, WIDTH};

use super::{in_shown, install, map_tile, shown, view_rect};
use crate::Files;
use crate::engine::Engine;
use crate::geom::{MAP_X, MAP_Y, SHOWN, VIEW_H, VIEW_W};
use crate::gfx::GfxState;
use crate::gfx::blit::{
    BitmapSource, DISSOLVE_SIDE, MapDissolve, UI, blit_back, blit_bitmap, copy_rect, dissolve_map,
    fill_colour,
};
use crate::state::State;

/// The colour of view cell (col, row) away from the border: columns and rows told apart.
fn cell_colour(col: usize, row: usize) -> u8 {
    (col % 4 + 4 * (row % 5) + 1) as u8
}

/// The border's tiles 0x1b0..=0x1b7 get colours 24..=31.
fn border_colour(tile: u16) -> u8 {
    (24 + tile - 0x1b0) as u8
}

/// The border tile the render draws over view cell (col, row), if any: the corners, then the
/// top, bottom, left and right edges.
fn border_at(col: usize, row: usize) -> Option<u16> {
    let (left, top, right, bottom) = (col == 0, row == 0, col == VIEW_W - 1, row == VIEW_H - 1);
    match (left, top, right, bottom) {
        (true, true, _, _) => Some(0x1b0),
        (_, true, true, _) => Some(0x1b2),
        (true, _, _, true) => Some(0x1b3),
        (_, _, true, true) => Some(0x1b5),
        (_, true, _, _) => Some(0x1b1),
        (_, _, _, true) => Some(0x1b4),
        (true, _, _, _) => Some(0x1b6),
        (_, _, true, _) => Some(0x1b7),
        _ => None,
    }
}

/// What the framed render leaves in view cell (col, row).
fn drawn_colour(col: usize, row: usize) -> u8 {
    border_at(col, row).map_or(cell_colour(col, row), border_colour)
}

/// A state whose view holds tile `1 + cell_colour` in each cell (colour = tile), its border tiles
/// opaque, no objects: the draw lists' base words written by view cell, `row * VIEW_W + col`.
fn window_state() -> State {
    let mut s = State::empty();
    let mut g = GfxState::default();
    for n in 1..=20u16 {
        install(&mut g, usize::from(n), map_tile(0, 16, move |_, _| n as u8));
    }
    for t in 0x1b0..=0x1b7u16 {
        install(
            &mut g,
            usize::from(t),
            map_tile(0, 16, move |_, _| border_colour(t)),
        );
    }
    s.gfx = g;
    let d = &mut s.objects.draw;
    d.base = (0..VIEW_W * VIEW_H)
        .map(|i| u16::from(cell_colour(i % VIEW_W, i / VIEW_W)))
        .collect();
    d.cell = vec![0; VIEW_W * VIEW_H];
    s
}

/// The frame the framed render of [`window_state`] publishes, dissolving when `dissolve`.
fn rendered(dissolve: bool) -> Framebuffer {
    let mut s = window_state();
    s.gfx.dissolve = u16::from(dissolve);
    let mut e = Engine::with_game(State::empty(), Files::default(), move |_s, io| async move {
        crate::gfx::render_window(&mut s, &io, true);
    });
    e.tick(&[], 0);
    e.frame().clone()
}

/// The screen shows the shown map and nothing else of the window: each of its pixels the cell's
/// tile or the border tile over it, none outside it. The bottom-right corner of [`SHOWN`] is in
/// the frame and shows the last cell's corner tile: with the view's columns and rows swapped
/// that corner would be past the frame's last row in Wide.
#[test]
fn the_screen_shows_exactly_the_shown_map_cell_by_cell() {
    let (x0, y0, x1, y1) = SHOWN;
    let [x0, y0, x1, y1] = [x0, y0, x1, y1].map(usize::from);
    assert!(x1 < WIDTH && y1 < HEIGHT, "the shown map in the frame");
    for dissolve in [false, true] {
        let fb = rendered(dissolve);
        let mut inside = 0;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let want = if in_shown(x, y) {
                    inside += 1;
                    let (col, row) = ((x - usize::from(MAP_X)) / 16, (y - usize::from(MAP_Y)) / 16);
                    drawn_colour(col, row)
                } else {
                    0
                };
                assert_eq!(fb.get(x, y), Some(want), "({x}, {y}), dissolve {dissolve}");
            }
        }
        println!("dissolve {dissolve}: {inside} pixels shown");
        assert_eq!(inside, (x1 + 1 - x0) * (y1 + 1 - y0));
        // The rect's corners show the corner tiles; one pixel outside each, nothing.
        assert_eq!(fb.get(x0, y0), Some(border_colour(0x1b0)));
        assert_eq!(fb.get(x1, y0), Some(border_colour(0x1b2)));
        assert_eq!(fb.get(x0, y1), Some(border_colour(0x1b3)));
        assert_eq!(fb.get(x1, y1), Some(border_colour(0x1b5)));
        for (x, y) in [(x0 - 1, y0), (x0, y0 - 1), (x1 + 1, y1), (x1, y1 + 1)] {
            assert_eq!(fb.get(x, y), Some(0), "({x}, {y}) outside");
        }
    }
}

/// The back buffer holds the whole view: the border's outer half too, and the window rect ends
/// at the view's last cell.
#[test]
fn the_back_buffer_holds_the_view_with_its_border() {
    let mut s = window_state();
    let io = crate::engine::Io::new(Files::default());
    crate::gfx::render_window(&mut s, &io, true);
    let (xs, ys) = view_rect();
    io.peek_back(|b| {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let want = if xs.contains(&x) && ys.contains(&y) {
                    drawn_colour((x - xs.start) / 16, (y - ys.start) / 16)
                } else {
                    0
                };
                assert_eq!(b.get(x, y), Some(want), "({x}, {y})");
            }
        }
    });
}

/// A feedback word with the full period of a `k`-bit register, found by search (not game data).
fn full_period_word(k: u32) -> u16 {
    (1u16 << (k - 1)..=u16::MAX >> (16 - k))
        .find(|&fb| {
            let (mut s, mut n) = (1u16, 0u32);
            loop {
                let carry = s & 1 != 0;
                s >>= 1;
                if carry {
                    s ^= fb;
                }
                n += 1;
                if s == 1 || n > 1 << k {
                    break;
                }
            }
            n == (1 << k) - 1
        })
        .expect("a full-period word")
}

/// Spec 13 Decision 7: the map's dissolve reveals every pixel of [`SHOWN`] once and nothing
/// else, in as many steps as the original's 160 × 160 order (so in the same time), each step at
/// most one pixel per tile.
#[test]
fn the_map_dissolve_reveals_every_shown_pixel_once_in_the_original_steps() {
    // The table holds a word per register width from 2; 160 × 160 needs 15 bits.
    let mut taps = vec![0u16; 15];
    taps[13] = full_period_word(15);
    let d = MapDissolve::new(&taps);
    let side = usize::from(DISSOLVE_SIDE);
    assert_eq!(d.steps(), side * side, "the original's steps");
    let (xs, ys) = shown();
    let tiles = xs.len().div_ceil(side) * ys.len().div_ceil(side);
    let mut seen = BTreeSet::new();
    for k in 0..d.steps() {
        let step: Vec<(u16, u16)> = d.step(k).collect();
        assert!(!step.is_empty() && step.len() <= tiles, "step {k}");
        for (x, y) in step {
            let p = (usize::from(x), usize::from(y));
            assert!(in_shown(p.0, p.1), "{p:?} outside the shown map");
            assert!(seen.insert(p), "{p:?} twice");
        }
    }
    println!(
        "{} pixels in {} steps, {tiles} tiles",
        seen.len(),
        d.steps()
    );
    assert_eq!(seen.len(), xs.len() * ys.len(), "every shown pixel");
    // The copy: exactly the shown map goes to the front.
    let mut back = Framebuffer::new();
    back.clear(9);
    let mut front = Framebuffer::new();
    dissolve_map(&taps, &back, &mut front);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let want = if in_shown(x, y) { 9 } else { 0 };
            assert_eq!(front.get(x, y), Some(want), "({x}, {y})");
        }
    }
}

/// Original: the map's dissolve is 0x1520e2 over (8, 8)..(0xa7, 0xa7) itself, one tile: step k
/// reveals (8, 8) plus the k-th pixel of the 160 × 160 order, and only it, for a full-period
/// word and for one that ends the register early.
#[cfg(not(feature = "wide"))]
#[test]
fn original_map_dissolve_steps_are_the_160_order_from_8_8() {
    let mut full = vec![0u16; 15];
    full[13] = full_period_word(15);
    for taps in [full, vec![0x1234u16; 15]] {
        let order = crate::gfx::blit::dissolve_order(DISSOLVE_SIDE, DISSOLVE_SIDE, &taps);
        let d = MapDissolve::new(&taps);
        assert_eq!(d.steps(), order.len());
        for (k, &(dx, dy)) in order.iter().enumerate() {
            let step: Vec<(u16, u16)> = d.step(k).collect();
            assert_eq!(step, [(8 + dx, 8 + dy)], "step {k}");
        }
        println!("{} steps checked", order.len());
    }
}

/// Spec 13 Design 4: the tile blitter, the fill, the rect copy and the bitmap blit starting in
/// the UI's columns write only on the UI's screen (in Original the frame is that screen; the
/// Amiga's wrap into the next row stays there).
#[test]
fn writes_from_the_ui_stay_on_the_ui_screen() {
    let mut gfx = GfxState::default();
    install(&mut gfx, 1, map_tile(0, 16, |_, _| 7));
    let mut fb = Framebuffer::new();
    // A tile at the UI's right edge and one at its bottom.
    blit_back(&gfx, &mut fb, 1, 312, 40);
    blit_back(&gfx, &mut fb, 1, 160, 192);
    // A fill from the UI to far past it, both ways.
    fill_colour(&mut fb, 300, 100, 0xfff0, 0xfff0, 7);
    // A copy of 4 bytes to the UI's last 2, and a bitmap 4 pixels past its right edge.
    let mut back = Framebuffer::new();
    back.clear(7);
    copy_rect(&back, &mut fb, 0, 0, 31, 3, 304, 60);
    let colours = std::array::from_fn(|i| i as u8);
    let bmp = vec![7u8; 20 * 4];
    blit_bitmap(
        &colours,
        &mut fb,
        (20, 4),
        &bmp,
        304,
        80,
        BitmapSource::Classic,
    );
    let outside = (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .filter(|&(x, y)| x >= UI.0 || y >= UI.1)
        .filter(|&(x, y)| fb.get(x, y) == Some(7))
        .count();
    let on_ui = (0..UI.1)
        .flat_map(|y| (0..UI.0).map(move |x| (x, y)))
        .filter(|&(x, y)| fb.get(x, y) == Some(7))
        .count();
    println!("{on_ui} pixels on the UI's screen, {outside} outside it");
    assert_eq!(outside, 0);
    assert!(fb.get(312, 40) == Some(7) && fb.get(319, 55) == Some(7));
    assert!(fb.get(160, 199) == Some(7) && fb.get(UI.0 - 1, UI.1 - 1) == Some(7));
}

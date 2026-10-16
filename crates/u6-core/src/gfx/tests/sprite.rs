//! The flight sprites (0x15265a / 0x152800 / 0x152876) and the rectangle fill 0x1521f4.

use u6_formats::gfx::Span;
use u6_render::Framebuffer;

use super::{GfxState, install, map_tile, object_tile};
use crate::gfx::blit::{fill_colour, fill_rect, set_plot_colour};
use crate::gfx::sprite::{draw, prepare, save};

/// A state whose sine table is 64 · sin(n / 128 turns) rounded (the shape of the game's).
fn gfx() -> GfxState {
    let mut g = GfxState::default();
    g.tables.sine = std::array::from_fn(|n| {
        (64.0 * (n as f64 * std::f64::consts::TAU / 128.0).sin()).round() as i8
    });
    g
}

/// A full-span tile: pixel (r, c) = r * 16 + c + 1 (mod 31) except the clear last column.
fn numbered() -> u6_formats::gfx::Tile {
    object_tile(Span::Full, |r, c| {
        (c < 15).then_some(((r * 16 + c) % 31 + 1) as u8)
    })
}

#[test]
fn unturned_and_half_turned_sprites_about_the_tile_centre() {
    let mut g = gfx();
    install(&mut g, 7, numbered());
    // Centre (128, 128) sixteenths: angle 0 gives the tile itself.
    prepare(&mut g, 2, 7, 0, 128, 128);
    let want = |r: usize, c: usize| {
        if c < 15 {
            ((r * 16 + c) % 31 + 1) as u8
        } else {
            0xff
        }
    };
    let s = &g.sprites.slots[2];
    assert!((0..256).all(|i| s[i] == want(i / 16, i % 16)));
    assert_eq!(g.sprites.centre, (1024, 1024));
    // Half a turn: both axes mirrored.
    prepare(&mut g, 3, 7, 0x40, 128, 128);
    let s = &g.sprites.slots[3];
    assert!((0..256).all(|i| s[i] == want(15 - i / 16, 15 - i % 16)));
    // An off-centre turn samples off the tile: those pixels are clear.
    prepare(&mut g, 4, 7, 0, 0, 0);
    assert_eq!(g.sprites.slots[4][0], 0xff);
    assert_eq!(g.sprites.slots[4][8 * 16 + 8], want(0, 0));
}

#[test]
fn map_tiles_and_slots_past_15_are_not_prepared() {
    let mut g = gfx();
    install(&mut g, 7, numbered());
    install(&mut g, 8, map_tile(0, 16, |_, _| 3));
    g.sprites.saved[1] = true;
    let before = g.sprites.clone();
    prepare(&mut g, 1, 8, 0, 128, 128);
    prepare(&mut g, 16, 7, 0, 128, 128);
    assert_eq!(g.sprites.slots, before.slots);
    assert!(g.sprites.saved[1], "the saved flag stays");
    prepare(&mut g, 1, 7, 0, 128, 128);
    assert!(
        !g.sprites.saved[1],
        "a prepared slot holds a sprite, not a background"
    );
}

#[test]
fn a_lower_record_gets_its_colours_rows_up_and_a_narrow_span_keeps_stale_columns() {
    let mut g = gfx();
    // Rows 4..8 only, colour 9 everywhere opaque.
    let mut t = object_tile(Span::Full, |r, _| (4..8).contains(&r).then_some(9));
    t.first_row = 4;
    t.rows = 4;
    install(&mut g, 7, t);
    prepare(&mut g, 0, 7, 0, 128, 128);
    let s = &g.sprites.slots[0];
    // Rows 4..8 are opaque; the planes went to rows 0..4, which are clear: no colour.
    assert_eq!(
        (s[4 * 16], s[7 * 16 + 3], s[3 * 16], s[8 * 16]),
        (0, 0, 0xff, 0xff)
    );
    // A right-span tile lands in columns 0..8; its rows keep columns 8..16 of the last tile.
    install(&mut g, 9, object_tile(Span::Right, |_, _| Some(5)));
    prepare(&mut g, 1, 9, 0, 128, 128);
    let s = &g.sprites.slots[1];
    assert_eq!(
        (s[0], s[7], s[8]),
        (5, 5, 0xff),
        "row 0 was clear in the scratch"
    );
    assert_eq!(
        s[5 * 16 + 8],
        0,
        "row 5, column 8: the previous tile's opaque colour 0"
    );
}

#[test]
fn draw_save_and_restore_a_block() {
    let mut g = gfx();
    install(&mut g, 7, numbered());
    prepare(&mut g, 5, 7, 0, 128, 128);
    let mut back = Framebuffer::new();
    back.clear(30);
    save(&mut g, &back, 5, 100, 50);
    assert!(g.sprites.saved[5]);
    g.sprites.saved[5] = false;
    draw(&g, &mut back, 5, 100, 50);
    // Top left at (92, 42); the clear column 15 leaves the background.
    assert_eq!(back.get(92, 42), Some(1));
    assert_eq!(
        back.get(92 + 14, 42 + 15),
        Some(((15 * 16 + 14) % 31 + 1) as u8)
    );
    assert_eq!(back.get(92 + 15, 42), Some(30));
    assert_eq!(back.get(91, 42), Some(30));
    // The saved block (24 pixels from byte column 100 / 8 - 1 = 11) goes back.
    g.sprites.saved[5] = true;
    draw(&g, &mut back, 5, 100, 50);
    assert!(back.pixels().iter().all(|&p| p == 30));
}

#[test]
fn fill_orders_the_corners_and_follows_the_plot_mode() {
    let mut g = gfx();
    let mut fb = Framebuffer::new();
    set_plot_colour(&mut g, 6);
    fill_rect(&g, &mut fb, 20, 12, 10, 10);
    assert_eq!(
        (
            fb.get(10, 10),
            fb.get(20, 12),
            fb.get(21, 12),
            fb.get(15, 13)
        ),
        (Some(6), Some(6), Some(0), Some(0))
    );
    assert_eq!(fb.pixels().iter().filter(|&&p| p == 6).count(), 11 * 3);
    g.plot_xor = true;
    set_plot_colour(&mut g, 3);
    fill_rect(&g, &mut fb, 10, 10, 10, 11);
    assert_eq!((fb.get(10, 10), fb.get(10, 11)), (Some(5), Some(5)));
    set_plot_colour(&mut g, 16);
    fill_rect(&g, &mut fb, 0, 0, 5, 5);
    assert_eq!(fb.get(0, 0), Some(0), "a colour past 15 plots nothing");
}

#[test]
fn an_empty_record_below_the_tile_prepares_a_clear_sprite() {
    // No rows, first row 17 (a malformed record the loader now refuses, 0x1526c2 clears
    // first_row × 16 bytes of the 256-byte scratch): the scratch is clear, nothing past it.
    let mut g = gfx();
    let mut t = object_tile(Span::Full, |_, _| None);
    t.first_row = 17;
    t.rows = 0;
    install(&mut g, 7, t);
    prepare(&mut g, 2, 7, 0, 128, 128);
    assert!(g.sprites.slots[2].iter().all(|&p| p == 0xff));
    // The file's own empty record (16, 0) clears exactly the scratch.
    let mut t = object_tile(Span::Full, |_, _| None);
    t.first_row = 16;
    t.rows = 0;
    install(&mut g, 8, t);
    prepare(&mut g, 3, 8, 0, 128, 128);
    assert!(g.sprites.slots[3].iter().all(|&p| p == 0xff));
}

#[test]
fn a_fill_over_wrapped_corners_stays_on_the_screen() {
    // x1 = -1 as a word: the rectangle reaches past the right edge; only row 0 is filled, from
    // x 300 to the edge (the byte address would run on into the rows below).
    let mut fb = Framebuffer::new();
    fill_colour(&mut fb, 300, 0, 0xffff, 0, 7);
    assert_eq!(fb.pixels().iter().filter(|&&p| p == 7).count(), 20);
    assert_eq!(
        (fb.get(300, 0), fb.get(319, 0), fb.get(0, 1)),
        (Some(7), Some(7), Some(0))
    );
    // Both corners wrapped on both axes: a bounded fill of the whole screen.
    fill_colour(&mut fb, 0, 0, 0xffff, 0xffff, 5);
    assert_eq!(fb.pixels().iter().filter(|&&p| p == 5).count(), 320 * 200);
}

/// Spec 07 HD0: a flight's saved block writes its sources in place, but when a clone of the
/// state shares them it copies them first (`Rc::make_mut`): the clone keeps the block it saved.
#[test]
fn a_saved_block_shared_with_a_clone_is_copied_before_it_is_written() {
    use std::rc::Rc;
    use u6_render::{Kind, Src};
    let mut g = gfx();
    let mut back = Framebuffer::new();
    back.enable_src();
    // The 24 × 16 block under a sprite at (64, 64): x 56..80, y 56..72, every pixel of record `id`.
    let draw = |back: &mut Framebuffer, id: u16| {
        for y in 56..72 {
            for x in 56..80 {
                let uv = ((x % 16) as u16, (y % 16) as u16);
                back.set_src(x, y, 3, super::plane::tile(id, 0, uv, 0));
            }
        }
    };
    let saved = |g: &GfxState| {
        let src: &Rc<[Src]> = g.sprites.under_src[3].0.as_ref().expect("sources saved");
        let ids: Vec<u16> = src.iter().map(|s| s.id).collect();
        assert!(src.iter().all(|s| s.kind == Kind::Tile));
        (Rc::as_ptr(src).cast::<Src>() as usize, ids)
    };
    let every = |id: u16| vec![id; 24 * 16];
    draw(&mut back, 1);
    save(&mut g, &back, 3, 64, 64);
    let (at, ids) = saved(&g);
    assert_eq!(ids, every(1));
    draw(&mut back, 2);
    save(&mut g, &back, 3, 64, 64);
    assert_eq!(saved(&g), (at, every(2)), "unshared: written in place");
    let clone = g.clone();
    assert_eq!(saved(&clone), (at, every(2)), "the clone shares them");
    draw(&mut back, 3);
    save(&mut g, &back, 3, 64, 64);
    assert_eq!(saved(&clone), (at, every(2)), "the clone keeps its block");
    let (moved, ids) = saved(&g);
    assert!(
        moved != at && ids == every(3),
        "shared: copied, then written"
    );
}

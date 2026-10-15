//! Spec 06 O2 on a synthetic world: the idle refresh with the render cache against the same
//! refresh without it, the screen, the back buffer and the whole `State` (the RNG included)
//! compared after every step.

use u6_formats::animdata::{AnimData, AnimEntry};
use u6_formats::gfx::{AnimOverlay, Rgb, Span};

use super::view::{HERO_BASE, prepare, world};
use super::{install, map_tile, object_tile};
use crate::Files;
use crate::engine::Io;
use crate::gfx::anim::set_frame;
use crate::state::State;

/// The animated tile, at world (20, 10) (window cell (6, 5)); its frames are records 100..104.
const ANIMATED: usize = 20;
/// The shore tile its record merges into, at (21, 10).
const SHORE: usize = 21;
/// A tile whose overlay draws while its masktype has bit 2, at (14, 10) and (22, 10).
const OVERLAID: usize = 22;
/// The hero's status bit that keeps its idle frames still (one of 0x16).
const STILL: u8 = 0x10;

/// The synthetic world of the view tests with what the idle refresh draws: an animated tile
/// (off at first), a shore merge from it, an overlay record, the hero's second frame, the two
/// cursor tiles, clock hands on the hero's tile (the clock type's base tile is the hero's),
/// a palette. The hero's idle frames are held still.
fn scene() -> State {
    let (mut s, mut g) = world();
    for k in 0..4 {
        let colour = move |r: usize, c: usize| ((r + 2 * c + k) % 31 + 1) as u8;
        install(&mut g, 100 + k, map_tile(0, 16, colour));
    }
    g.tables.shore_src[0] = (ANIMATED * 4) as u16;
    g.tables.shore_dst[0] = (SHORE * 4) as u16;
    // Block 2: merge 0 takes the left half of all 16 rows; the other 31 point past the block.
    let mut b2 = vec![0u8; 64];
    for i in 1..32 {
        b2[2 * i..2 * i + 2].copy_from_slice(&0xF000u16.to_le_bytes());
    }
    let rec = b2.len() as u16;
    b2.extend([0, 16]);
    for _ in 0..16 {
        b2.extend([0xFF, 0x00]);
    }
    b2[0..2].copy_from_slice(&rec.wrapping_sub(0x400).to_le_bytes());
    g.animmask.block2 = b2;
    g.overlays[OVERLAID] = Some(AnimOverlay {
        first_row: 4,
        rows: 4,
        frames: (0..8).map(|f| vec![0x8000u16 >> (2 * f); 4]).collect(),
    });
    install(
        &mut g,
        usize::from(HERO_BASE) + 1,
        object_tile(Span::Full, |r, c| (r == 4 && c == 4).then_some(29)),
    );
    install(
        &mut g,
        0x16c,
        object_tile(Span::Full, |r, c| (r == c).then_some(27)),
    );
    install(
        &mut g,
        0x16d,
        object_tile(Span::Full, |r, c| (r + c == 15).then_some(26)),
    );
    g.tables.window_colour = 13;
    g.tables.clock_colour = 6;
    g.tables.dir_dx = [0, 1, 1, 1, 0, -1, -1, -1];
    g.tables.dir_dy = [-1, -1, 0, 1, 1, 1, 0, -1];
    g.tables.hour_dir = [0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7, 7];
    s.gfx = g;
    s.tables.anim = AnimData {
        count: 1,
        slots: vec![AnimEntry {
            tile_to_animate: ANIMATED as u16,
            first_anim_frame: 100,
            and_mask: 0x0c,
            shift: 2,
        }],
    };
    s.map.anim_enabled = vec![0];
    s.tables.basetile.base[0x9f] = HERO_BASE;
    for (i, c) in s.tables.palette.iter_mut().enumerate() {
        let v = i as u8;
        *c = Rgb {
            r: v * 8,
            g: 255 - v * 8,
            b: 0x40,
        };
    }
    s.actors.attrs.status[1] |= STILL;
    s
}

/// What step `i` changes before its refresh, in the state and on the buffers.
fn edit(s: &mut State, io: &Io, i: u32) {
    let input = &mut s.ui.input;
    match i {
        40 => s.map.anim_enabled = vec![1],
        120 => s.gfx.masktype[OVERLAID] |= 4,
        140 => s.gfx.masktype[OVERLAID] &= !4,
        150 => s.map.anim_enabled = vec![0],
        // The cursor on an edge cell (with its border tile), then inside.
        160 => {
            input.target_pending = 1;
            input.targeting = 1;
            input.cursor = (0, 4);
            input.range = 1;
        }
        170 => input.cursor = (4, 4),
        175 => input.range = -1,
        // The cursor's owner idle again: the render's 0x106932 call clears `targeting`.
        180 => input.target_pending = 0,
        190 => s.tables.palette[5] = Rgb { r: 1, g: 2, b: 3 },
        200 => s.gfx.sparkle = 1,
        210 => s.gfx.sparkle = 0,
        215 => s.gfx.dissolve = 1,
        220 => io.draw_back(|b| b.set(30, 30, 7)),
        225 => io.draw(|f| f.set(40, 40, 9)),
        230 => {
            s.objects.location[1].x += 1;
            prepare(s);
        }
        240 => s.actors.calendar.minute = 30,
        245 => s.actors.calendar.hour = 7,
        250 => s.actors.attrs.status[1] &= !STILL,
        _ => {}
    }
}

/// What a cached refresh drew of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drawn {
    /// Nothing: every box is as the render would draw it.
    Nothing,
    /// That many boxes.
    Boxes(u64),
    /// The whole window, as without the cache.
    All,
}

/// Steps 0..320 of the idle refresh with the cache (`on`) and without (`off`), compared after
/// each; returns what each cached refresh drew.
fn run(on: &mut State, off: &mut State) -> Vec<Drawn> {
    let (io_on, io_off) = (Io::new(Files::default()), Io::new(Files::default()));
    io_off.set_window_cache(false);
    let screen = |io: &Io| io.draw(|f| (f.pixels().to_vec(), f.palette));
    let back = |io: &Io| io.peek_back(|b| (b.pixels().to_vec(), b.palette));
    let counts = |s: &State| {
        let c = &s.gfx.cache;
        (c.window_skips, c.window_partial, c.boxes_redrawn)
    };
    let mut drawn = Vec::new();
    for i in 0..320u32 {
        let before = counts(on);
        for (s, io) in [(&mut *on, &io_on), (&mut *off, &io_off)] {
            edit(s, io, i);
            // The idle poll's: idle frames due, then the overlay frame steps (0x105c8e).
            s.gfx.actor_anim_due = 1;
            set_frame(&mut s.gfx, i as u8);
            crate::gfx::refresh_view(s, io);
        }
        let after = counts(on);
        drawn.push(if after.0 > before.0 {
            Drawn::Nothing
        } else if after.1 > before.1 {
            Drawn::Boxes(after.2 - before.2)
        } else {
            Drawn::All
        });
        assert!(screen(&io_on) == screen(&io_off), "step {i}: the screen");
        assert!(back(&io_on) == back(&io_off), "step {i}: the back buffer");
        assert!(on.rng == off.rng, "step {i}: the RNG");
        assert!(on.gfx == off.gfx, "step {i}: the graphics state");
        assert!(*on == *off, "step {i}: the whole State");
    }
    let c = &off.gfx.cache;
    assert_eq!(
        (c.window_skips, c.window_partial, c.shore_skips),
        (0, 0, 0),
        "off: every refresh renders the whole window and merges"
    );
    drawn
}

/// The cache draws exactly the boxes whose draws read a changed value, and the frames and the
/// state stay those of the every-time render: idle with nothing changing, the animation moving
/// a tile's frame (and the shore merge after it), an overlay frame stepping, the cursor shown
/// on an edge cell, moved, turned and taken away, a palette change, the sparkle (RNG draws),
/// the dissolve, a draw on the back buffer, a draw on the screen's window, an actor moving,
/// the clock's time, the actors' idle frames (RNG draws, lists rebuilt).
#[test]
fn the_cached_refresh_draws_what_the_every_time_refresh_draws() {
    use Drawn::{All, Boxes, Nothing};
    let mut on = scene();
    let mut off = on.clone();
    let shore_before = on.gfx.records[SHORE].pixels.clone();
    let drawn = run(&mut on, &mut off);
    let count = |r: std::ops::Range<usize>, f: &dyn Fn(Drawn) -> bool| {
        drawn[r].iter().filter(|&&d| f(d)).count()
    };
    let nothing = |d| d == Nothing;
    let some = |d| matches!(d, Boxes(_));
    let phases = [
        ("idle", 2..40),
        ("animation", 41..120),
        ("overlay", 121..140),
        ("cursor", 160..190),
        ("sparkle", 200..211),
        ("idle frames", 250..320),
    ];
    for (name, r) in &phases {
        let boxes: u64 = drawn[r.clone()]
            .iter()
            .map(|d| if let Boxes(n) = d { *n } else { 0 })
            .sum();
        println!(
            "{name}: of {} refreshes {} drew nothing, {} drew {boxes} boxes, {} all",
            r.len(),
            count(r.clone(), &nothing),
            count(r.clone(), &some),
            count(r.clone(), &|d| d == All)
        );
    }
    let c = &on.gfx.cache;
    println!(
        "shore merges left out: {}; renders: {} nothing, {} partial ({} boxes)",
        c.shore_skips, c.window_skips, c.window_partial, c.boxes_redrawn
    );
    assert_eq!(drawn[0], All, "the first render");
    assert_eq!(drawn[1], Boxes(1), "the first merge: the shore's box");
    assert_eq!(count(2..40, &nothing), 38, "idle: nothing changes");
    // The animated frame moves every 4 counts: its tile's box and, after the merge, the
    // shore's are drawn, the next 3 refreshes draw nothing.
    assert_eq!(count(41..120, &nothing), 58, "animation");
    assert!(
        drawn[40..120]
            .iter()
            .all(|&d| d == Nothing || d == Boxes(1) || d == Boxes(2)),
        "animation: the tile and the shore only"
    );
    // The overlaid tile is in two cells (one on the edge, with its border tile); every fourth
    // refresh the animated tile and the shore too.
    assert_eq!(
        count(121..140, &|d| d == Boxes(2) || d == Boxes(4)),
        19,
        "the overlay's boxes every refresh"
    );
    assert_eq!(drawn[160], Boxes(1), "the cursor on its edge cell");
    assert_eq!(drawn[170], Boxes(2), "the cursor moved: both cells");
    assert_eq!((drawn[175], drawn[180]), (Boxes(1), Boxes(1)));
    assert_eq!(
        drawn[190], Nothing,
        "a palette change: loaded, nothing drawn"
    );
    assert_eq!(
        count(200..211, &|d| d == All),
        11,
        "the sparkle, and right after it"
    );
    assert_eq!(
        (drawn[215], drawn[216]),
        (All, All),
        "the dissolve, and after it"
    );
    assert_eq!(drawn[220], All, "the back buffer was drawn on");
    assert_eq!(
        drawn[225], Nothing,
        "the screen's window drawn on: presented again"
    );
    assert_eq!(drawn[230], Boxes(2), "the hero moved: both cells");
    assert_eq!((drawn[240], drawn[245]), (Boxes(1), Boxes(1)), "the clock");
    assert!(
        count(250..320, &some) > 0 && count(250..320, &nothing) > 0,
        "idle frames: a frame changed, other steps changed nothing"
    );
    assert!(on.gfx.cache.shore_skips > 0, "unchanged merges left out");
    assert!(
        on.gfx.records[SHORE].pixels != shore_before,
        "the merges that ran changed the shore"
    );
}

/// The renders without a memo: a clone of the state does not take the cache with it (another
/// `Io`'s back buffer); the cursor outside the cells and an overlay whose rows pass its tile
/// (draws that leave their box) render the whole window every time. A crossed shore merge (a
/// target record that is also a source) is never left out.
#[test]
fn the_cache_stands_aside_where_it_cannot_stand_for_the_render() {
    let renders = |s: &State| {
        let c = &s.gfx.cache;
        c.window_skips + c.window_partial
    };
    let mut s = scene();
    let io = Io::new(Files::default());
    for _ in 0..4 {
        crate::gfx::refresh_view(&mut s, &io);
    }
    assert_eq!(renders(&s), 3, "after the first, from the memo");
    let mut copy = s.clone();
    assert_eq!(renders(&copy), 0, "a clone starts empty");
    let other = Io::new(Files::default());
    crate::gfx::refresh_view(&mut copy, &other);
    assert_eq!(renders(&copy), 0, "another Io: everything");
    let input = &mut copy.ui.input;
    (input.target_pending, input.targeting, input.cursor) = (1, 1, (11, 2));
    for _ in 0..3 {
        crate::gfx::refresh_view(&mut copy, &other);
    }
    assert_eq!(
        renders(&copy),
        0,
        "the cursor outside the cells: everything"
    );
    let mut tall = scene();
    tall.gfx.masktype[OVERLAID] |= 4;
    if let Some(ov) = tall.gfx.overlays[OVERLAID].as_mut() {
        ov.first_row = 14;
    }
    for _ in 0..3 {
        crate::gfx::refresh_view(&mut tall, &io);
    }
    assert_eq!(renders(&tall), 0, "overlay rows past the tile: everything");
    // Merge 1 writes the record merge 0 reads.
    s.gfx.tables.shore_src[1] = (SHORE * 4) as u16;
    s.gfx.tables.shore_dst[1] = (ANIMATED * 4) as u16;
    let rec = s.gfx.animmask.block2[0..2].to_vec();
    s.gfx.animmask.block2[2..4].copy_from_slice(&rec);
    let skips = s.gfx.cache.shore_skips;
    for _ in 0..8 {
        crate::gfx::refresh_view(&mut s, &io);
    }
    assert_eq!(s.gfx.cache.shore_skips, skips, "crossed: every merge runs");
}

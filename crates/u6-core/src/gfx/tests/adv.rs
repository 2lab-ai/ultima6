//! Spec 06 O2, adversarial (from the review of the window cache): two equal synthetic `State`s
//! and two `Io`s, the cache on and off, the same seeded random edits on both before every idle
//! refresh (window words and lists, masktypes, substitutions on map, object and border tiles,
//! the shore merges set once, the animation modes, the clock's tile and time, the cursor, the
//! overlay frame, the plot state, XOR plots, the sparkle, the dissolve, the window and pause
//! flags, the palette, draws on either buffer, a clone of the state taking over, a new `Io`, a
//! foreign render on the same buffers), then the screen, the back buffer, the RNG and the whole
//! `State` compared. No game data: every tile, table and mask is made here.

use u6_formats::animdata::{AnimData, AnimEntry};
use u6_formats::gfx::{AnimOverlay, Rgb, Span};

use super::{at_map, install, map_tile, object_tile};
use crate::Files;
use crate::engine::Io;
use crate::geom::{VIEW_H, VIEW_W};
use crate::gfx::anim::set_frame;
use crate::state::State;

/// A xorshift64 stream (the edits' dice).
#[derive(Clone)]
pub(super) struct X(pub(super) u64);

impl X {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    /// Per mille.
    pub(super) fn pm(&mut self, p: u64) -> bool {
        self.below(1000) < p
    }
}

const MAPS: usize = 48;
const OBJ0: usize = 64;
const NOBJ: usize = 18;
/// The record whose overlay leaves its box.
const TALL: usize = 47;

fn mask_record(b2: &mut Vec<u8>, skip: u8, rows: &[u16]) -> u16 {
    let off = b2.len() as u16;
    b2.push(skip);
    b2.push(rows.len() as u8);
    for r in rows {
        b2.extend(r.to_be_bytes());
    }
    off.wrapping_sub(0x400)
}

/// Map records 0..48 (rows of varied extent), object records 64..82 of the three spans with
/// colour-0 pixels (outline paths), border and cursor records, overlays with repeated frames
/// and one ([`TALL`]) whose rows pass its tile, the clock's tables, three animation slots, a
/// palette; and the mask words of four shore mask records (overlapping, partial, an odd skip)
/// and one past the block.
pub(super) fn scene() -> (State, Vec<u16>) {
    let mut s = State::empty();
    let mut g = crate::gfx::GfxState::default();
    for n in 0..MAPS {
        let first = ((n % 5) * 2) as u8;
        let rows = 16 - first - (n % 3) as u8;
        install(
            &mut g,
            n,
            map_tile(first, rows, move |r, c| {
                ((r * 3 + c * 5 + n) % 31 + 1) as u8
            }),
        );
    }
    for k in 0..NOBJ {
        let span = [Span::Full, Span::Left, Span::Right][k % 3];
        install(
            &mut g,
            OBJ0 + k,
            object_tile(span, move |r, c| {
                ((r * 7 + c * 3 + k) % 4 == 0).then_some(if (r + k) % 3 == 0 {
                    0
                } else {
                    ((r + c + k) % 31 + 1) as u8
                })
            }),
        );
    }
    for (i, t) in (0x1b0..=0x1b7).enumerate() {
        install(
            &mut g,
            t,
            map_tile(0, 16, move |r, c| ((r ^ c) + i) as u8 % 31 + 1),
        );
    }
    install(
        &mut g,
        0x16c,
        object_tile(Span::Full, |r, c| (r == c).then_some(27)),
    );
    install(
        &mut g,
        0x16d,
        object_tile(Span::Right, |r, c| (r + c == 15).then_some(0)),
    );
    // Overlays with repeated frames (3 distinct patterns over 8 frames).
    let pats = [0x8001u16, 0x0ff0, 0x8001];
    for r in (0..MAPS).step_by(4).chain([OBJ0 + 1, OBJ0 + 4, OBJ0 + 8]) {
        let rows = 3 + (r % 4) as u8;
        g.overlays[r] = Some(AnimOverlay {
            first_row: (r % 8) as u8,
            rows,
            frames: (0..8)
                .map(|f| vec![pats[(f * 5 + r) % 3] >> (f % 2); usize::from(rows)])
                .collect(),
        });
        g.masktype[r] |= 4;
    }
    g.overlays[TALL] = Some(AnimOverlay {
        first_row: 13,
        rows: 4,
        frames: (0..8).map(|f| vec![0xffffu16 >> f; 4]).collect(),
    });
    g.masktype[TALL] |= 4;
    g.tables.window_colour = 13;
    g.tables.clock_colour = 6;
    g.tables.dir_dx = [0, 1, 1, 1, 0, -1, -1, -1];
    g.tables.dir_dy = [-1, -1, 0, 1, 1, 1, 0, -1];
    g.tables.hour_dir = [0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7, 7];
    // Mask records: A, B overlap A, C partial, D odd skip.
    let mut b2 = vec![0u8; 64];
    let mut words = vec![
        mask_record(&mut b2, 0, &[0xFF00; 16]),
        mask_record(&mut b2, 0, &[0x0FF0; 16]),
        mask_record(&mut b2, 4, &[0xAAAA; 6]),
        mask_record(&mut b2, 3, &[0xFFFF; 4]),
    ];
    words.push(0xF000);
    g.animmask.block2 = b2;
    g.window_shown = 1;
    s.gfx = g;
    s.tables.anim = AnimData {
        count: 3,
        slots: vec![
            AnimEntry {
                tile_to_animate: 10,
                first_anim_frame: 40,
                and_mask: 0x0c,
                shift: 2,
            },
            AnimEntry {
                tile_to_animate: (OBJ0 + 2) as u16,
                first_anim_frame: (OBJ0 + 10) as u16,
                and_mask: 0x03,
                shift: 0,
            },
            AnimEntry {
                tile_to_animate: 20,
                first_anim_frame: 21,
                and_mask: 0x01,
                shift: 0,
            },
        ],
    };
    s.map.anim_enabled = vec![1, 2, 1];
    s.tables.basetile.base = vec![0; 1024];
    s.tables.basetile.base[0x9f] = (OBJ0 + 5) as u16;
    for (i, c) in s.tables.palette.iter_mut().enumerate() {
        let v = i as u8;
        *c = Rgb {
            r: v.wrapping_mul(8),
            g: 255 - v,
            b: 0x40,
        };
    }
    (s, words)
}

/// A random window word: a map, object, animated, cursor or border tile with random high bits
/// (the outline modes among them).
fn word(x: &mut X) -> u16 {
    let t = match x.below(10) {
        0..=3 => x.below(MAPS as u64 - 1) as u16, // never TALL here
        4..=7 => (OBJ0 as u64 + x.below(NOBJ as u64)) as u16,
        8 => [10u16, 20, 40, 41, 42, 43][x.below(6) as usize],
        _ => [0x16c, 0x16d, 0x1b0, 0x1b5][x.below(4) as usize],
    };
    let bits = [0u16, 0, 0, 0x0800, 0x1000, 0x1800, 0x2000, 0x8800][x.below(8) as usize];
    t | bits
}

/// The view's cells (121 in Original), by view cell `row * VIEW_W + col` (spec 13 C2).
pub(super) const CELLS: usize = VIEW_W * VIEW_H;

/// Writes per-cell base words and object lists into the draw lists ([`CELLS`] of each).
fn write_lists(s: &mut State, base: &[u16], lists: &[Vec<u16>]) {
    let d = &mut s.objects.draw;
    d.base.clear();
    d.base.extend_from_slice(base);
    d.cell = vec![0; base.len()];
    d.node_next.fill(0);
    d.node_tile.fill(0);
    let mut n = 1usize;
    for (b, l) in lists.iter().enumerate() {
        let mut prev: Option<usize> = None;
        for &w in l {
            if n >= 255 {
                break;
            }
            d.node_tile[n] = w;
            match prev {
                None => d.cell[b] = n as _,
                Some(p) => d.node_next[p] = n as _,
            }
            prev = Some(n);
            n += 1;
        }
    }
}

/// Each cell's base word and object list, as the edits keep them.
pub(super) struct Lists {
    pub(super) base: Vec<u16>,
    pub(super) lists: Vec<Vec<u16>>,
}

/// Step `step`'s random edits (the same stream on both sides).
pub(super) fn edit(s: &mut State, io: &Io, x: &mut X, l: &mut Lists, words: &[u16], step: u32) {
    if step == 0 || x.pm(15) {
        for b in 0..CELLS {
            l.base[b] = word(x) & if x.pm(800) { 0x7ff } else { 0xffff };
            l.lists[b] = (0..x.below(3)).map(|_| word(x)).collect();
        }
    } else if x.pm(400) {
        for _ in 0..1 + x.below(3) {
            let b = x.below(CELLS as u64) as usize;
            if x.pm(500) {
                l.base[b] = word(x);
            } else {
                l.lists[b] = (0..x.below(4)).map(|_| word(x)).collect();
            }
        }
    }
    if x.pm(8) {
        // The overlay that leaves its box, somewhere.
        l.base[x.below(CELLS as u64) as usize] = TALL as u16;
    }
    write_lists(s, &l.base, &l.lists);
    let g = &mut s.gfx;
    if x.pm(60) {
        let t = x.below(OBJ0 as u64 + NOBJ as u64) as usize;
        g.masktype[t] ^= 4;
    }
    if x.pm(20) {
        let t = x.below(OBJ0 as u64 + NOBJ as u64) as usize;
        g.masktype[t] = x.below(8) as u8;
    }
    if x.pm(20) {
        let (t, src) = (x.below(80) as u16, x.below(80) as u16);
        crate::gfx::anim::substitute(g, t, src);
    }
    if x.pm(10) {
        // A border tile drawing another record (the border's words are in the key).
        let (t, src) = (0x1b0 + x.below(8) as u16, x.below(80) as u16);
        crate::gfx::anim::substitute(g, t, src);
    }
    if step == 0 {
        // The shore merges (static after the load, as the game's tables are), crossed ones
        // among them.
        g.tables.shore_src = [0; 32];
        g.tables.shore_dst = [0; 32];
        let mut b2w = [0xF000u16; 32];
        let k = x.below(7) as usize;
        for (i, mask) in b2w.iter_mut().enumerate().take(k) {
            let pick = |x: &mut X| {
                if x.pm(100) {
                    (OBJ0 as u64 + x.below(NOBJ as u64)) as u16
                } else {
                    x.below(MAPS as u64) as u16
                }
            };
            let (src, mut dst) = (pick(x), pick(x));
            if x.pm(150) {
                dst = src;
            }
            g.tables.shore_src[i] = src * 4 + if x.pm(30) { 1 } else { 0 };
            g.tables.shore_dst[i] = dst * 4;
            *mask = words[x.below(words.len() as u64) as usize];
        }
        for (i, w) in b2w.iter().enumerate() {
            g.animmask.block2[2 * i..2 * i + 2].copy_from_slice(&w.to_le_bytes());
        }
    }
    if x.pm(30) {
        s.map.anim_enabled = (0..3).map(|_| x.below(3) as u8).collect();
    }
    if x.pm(15) {
        s.tables.basetile.base[0x9f] = match x.below(4) {
            0 => x.below(MAPS as u64) as u16,
            1 => 0x9000,
            _ => (OBJ0 as u64 + x.below(NOBJ as u64)) as u16,
        };
    }
    let cal = &mut s.actors.calendar;
    if x.pm(80) {
        cal.minute = x.below(60) as u8;
    }
    if x.pm(40) {
        cal.hour = x.below(24) as u8;
    }
    let t = &mut s.gfx.tables;
    if step == 0 {
        // Static after the load; one direction leaves the box for half the seeds.
        if x.pm(500) {
            t.dir_dx[7] = 20;
            t.dir_dy[7] = -20;
        }
    }
    let input = &mut s.ui.input;
    if x.pm(60) {
        input.target_pending = x.below(2) as u16;
        input.targeting = x.below(3) as u8;
        input.cursor_by_tab = x.pm(200);
        let (w, h) = (VIEW_W as u64 + 4, VIEW_H as u64 + 4);
        input.cursor = (x.below(w) as i8 - 2, x.below(h) as i8 - 2);
        input.range = if x.pm(500) { -1 } else { 2 };
    }
    let g = &mut s.gfx;
    set_frame(g, x.below(8) as u8);
    if x.pm(30) {
        g.plot_colour = x.below(20) as u16;
    }
    if x.pm(30) {
        g.plot_back = !g.plot_back;
    }
    g.plot_xor = x.pm(10);
    g.sparkle = u8::from(x.pm(15));
    if x.pm(15) {
        g.dissolve = 1;
    }
    if x.pm(10) {
        g.window_shown ^= 1;
    }
    if x.pm(10) {
        g.anim_paused ^= 1;
    }
    g.actor_anim_due = u16::from(x.pm(300));
    if x.pm(15) {
        let i = x.below(s.tables.palette.len() as u64) as usize;
        s.tables.palette[i] = Rgb {
            r: x.below(256) as u8,
            g: 1,
            b: 2,
        };
    }
    // A pixel of the window (its cells, 176 × 176 in Original), on either buffer.
    let (w, h) = (VIEW_W as u64 * 16, VIEW_H as u64 * 16);
    if x.pm(10) {
        let (px, py, c) = (x.below(w) as usize, x.below(h) as usize, x.below(32) as u8);
        let (px, py) = at_map(px, py);
        io.draw_back(|b| b.set(px, py, c));
    }
    if x.pm(20) {
        let (px, py, c) = (x.below(w) as usize, x.below(h) as usize, x.below(32) as u8);
        let (px, py) = at_map(px, py);
        io.draw(|f| f.set(px, py, c));
    }
}

/// `steps` refreshes of seed `seed`, compared after each; returns the cached refreshes that drew
/// nothing, those that drew some cells, and the cells they drew.
fn run(seed: u64, steps: u32) -> (u64, u64, u64) {
    let (mut on, words) = scene();
    let mut off = on.clone();
    let (mut io_on, mut io_off) = (Io::new(Files::default()), Io::new(Files::default()));
    io_off.set_window_cache(false);
    let mut rng = X(seed | 1);
    let mut l = Lists {
        base: vec![0; CELLS],
        lists: vec![Vec::new(); CELLS],
    };
    let counts = |s: &State| {
        let c = &s.gfx.cache;
        (c.window_skips, c.window_partial, c.boxes_redrawn)
    };
    let mut total = (0, 0, 0);
    for i in 0..steps {
        let mut x_on = rng.clone();
        let mut l_on = Lists {
            base: l.base.clone(),
            lists: l.lists.clone(),
        };
        edit(&mut on, &io_on, &mut x_on, &mut l_on, &words, i);
        edit(&mut off, &io_off, &mut rng, &mut l, &words, i);
        assert!(rng.0 == x_on.0, "the same edits");
        if rng.pm(5) {
            // A clone of the State takes over (no window memo, the versions kept).
            on = on.clone();
            off = off.clone();
        }
        if rng.pm(3) {
            // Another Io (blank buffers).
            io_on = Io::new(Files::default());
            io_off = Io::new(Files::default());
            io_off.set_window_cache(false);
        }
        if rng.pm(5) {
            // Another state renders on the same buffers in between.
            let mut other_on = on.clone();
            let mut other_off = off.clone();
            other_on.objects.draw.base[60] ^= 1;
            other_off.objects.draw.base[60] ^= 1;
            crate::gfx::refresh_view(&mut other_on, &io_on);
            crate::gfx::refresh_view(&mut other_off, &io_off);
        }
        // Counted on the state that refreshes (a clone above started its counts at 0).
        let before = counts(&on);
        crate::gfx::refresh_view(&mut on, &io_on);
        crate::gfx::refresh_view(&mut off, &io_off);
        let after = counts(&on);
        total.0 += after.0 - before.0;
        total.1 += after.1 - before.1;
        total.2 += after.2 - before.2;
        let screen = |io: &Io| io.draw(|f| (f.pixels().to_vec(), f.palette));
        let back = |io: &Io| io.peek_back(|b| (b.pixels().to_vec(), b.palette));
        let differ = |a: &[u8], b: &[u8]| a.iter().zip(b).filter(|(p, q)| p != q).count();
        let (s_on, s_off) = (screen(&io_on), screen(&io_off));
        let (b_on, b_off) = (back(&io_on), back(&io_off));
        assert!(
            s_on == s_off && b_on == b_off,
            "seed {seed} step {i}: {} screen and {} back-buffer pixels differ (palettes {} {})",
            differ(&s_on.0, &s_off.0),
            differ(&b_on.0, &b_off.0),
            s_on.1 == s_off.1,
            b_on.1 == b_off.1
        );
        assert!(on.rng == off.rng, "seed {seed} step {i}: the RNG");
        assert!(
            on.gfx == off.gfx,
            "seed {seed} step {i}: the graphics state"
        );
        assert!(on == off, "seed {seed} step {i}: the whole State");
    }
    total
}

/// Seeds of 400 steps the gate runs (the review ran 60; raise it for a deeper run).
const SEEDS: u64 = 4;

#[test]
fn adversarial_cache_equivalence() {
    let seeds = SEEDS;
    let mut total = (0, 0, 0);
    for seed in 0..seeds {
        let r = run(0x9E37_79B9_7F4A_7C15 ^ (seed * 0x1000_0001), 400);
        total = (total.0 + r.0, total.1 + r.1, total.2 + r.2);
    }
    println!(
        "adversarial: {seeds} seeds x 400 steps: {} refreshes drew nothing, {} drew {} cells",
        total.0, total.1, total.2
    );
    assert!(total.0 > 0 && total.1 > 0, "the cache was exercised");
}

//! Spec 07 HD0 parity and an independent check of the plane, on the adversarial edits of the
//! window cache (`adv`): the colours, the palettes, the RNG, the `State` and the cache's
//! decisions are the same with the source plane and without it, over three bare `Io` runs and
//! over two paired engines (every buffer and the pointer sprite, flights, a text scroll, quakes
//! with their playback). In the runs with the plane, every attributed pixel equals its record's
//! pixel at (u, v) when its version is current, and every pixel a record drawn in a window cell
//! writes is attributed: checks that read the records and the window's lists, not the plane's
//! writers. No game data.

use std::collections::BTreeSet;

use u6_formats::font::BitFont;
use u6_formats::gfx::{Span, TileKind};
use u6_render::{Framebuffer, Kind};

use super::adv::{self, Lists, X};
use crate::Files;
use crate::engine::{Engine, InputEvent, Io};
use crate::state::State;

/// What the window cache decided so far: renders left out, renders of some boxes, boxes drawn.
fn decisions(s: &State) -> (u64, u64, u64) {
    let c = &s.gfx.cache;
    (c.window_skips, c.window_partial, c.boxes_redrawn)
}

/// The receipt's rule on synthetic data: each attributed pixel of `fb` without flags whose
/// version is its record's current one has its record's colour at (u, v). Only records are
/// drawn in these runs. Returns the pixels checked.
fn check_images(fb: &Framebuffer, s: &State, what: &str) -> usize {
    let mut checked = 0;
    for (i, src) in fb.src().expect("a plane").iter().enumerate() {
        match src.kind {
            Kind::Classic => continue,
            Kind::Tile => {}
            other => panic!("{what}: pixel {i} names a {other:?}; none is drawn"),
        }
        let id = usize::from(src.id);
        if src.flags != 0 || src.ver != s.gfx.cache.version(id) {
            continue;
        }
        let (u, v) = (usize::from(src.u), usize::from(src.v));
        assert!(u < 16 && v < 16, "{what}: pixel {i} at ({u}, {v})");
        let want = s.gfx.records[id].pixels[v * 16 + u];
        assert_eq!(fb.pixels()[i], want, "{what}: pixel {i} against its record");
        checked += 1;
    }
    checked
}

/// Every pixel of the back buffer's window cells that a base or object record drawn there
/// writes (an opaque pixel; in outline mode an opaque colour-0 one, 8 columns left for a
/// right-span record) is attributed, but the clock's hands. Only for a refresh that rendered
/// without the sparkle. Returns the pixels checked.
fn check_coverage(back: &Framebuffer, s: &State, what: &str) -> usize {
    let g = &s.gfx;
    if g.window_shown == 0 || g.sparkle != 0 {
        return 0;
    }
    let w = crate::gfx::seams::view_window(s);
    let clock = s.tables.basetile.base.get(0x9f).copied();
    let mut hands = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for row in 0..11 {
        for col in 0..11 {
            let (x0, y0) = (col * 16, row * 16);
            let objects = &w.objects[row * 11 + col];
            for (k, &word) in std::iter::once(&w.tiles[row][col])
                .chain(objects)
                .enumerate()
            {
                if k > 0 && Some(word & 0x7ff) == clock {
                    let cal = &s.actors.calendar;
                    let p = crate::gfx::view::clock_points(cal, &g.tables, x0 as u16, y0 as u16);
                    hands.extend(p.map(|(x, y)| (usize::from(x), usize::from(y))));
                }
                let t = usize::from(word & 0x7ff);
                let rec = &g.records[usize::from(g.record_of[t])];
                let outline = g.masktype[t] & 3 != 0 && word & 0x1800 != 0;
                let shift = match rec.kind {
                    TileKind::Object(Span::Right) if outline => 8,
                    _ => 0,
                };
                for i in 0..256 {
                    let (r, c) = (i / 16, i % 16);
                    let drawn =
                        !rec.transparent[i] && (!outline || rec.pixels[i] == 0 && c >= shift);
                    if drawn {
                        covered.insert((x0 + c - shift, y0 + r));
                    }
                }
            }
        }
    }
    for &(x, y) in covered.difference(&hands) {
        let src = back.src_at(x, y).expect("on the screen");
        assert!(
            src.kind != Kind::Classic,
            "{what}: ({x}, {y}) is drawn from a record"
        );
    }
    covered.len()
}

/// The three runs of [`plane_parity`]: the plane off with the window cache, on with it, on
/// without it.
fn three_ios() -> [Io; 3] {
    let ios = [0, 1, 2].map(|_| Io::new(Files::default()));
    ios[1].enable_src_plane();
    ios[2].enable_src_plane();
    ios[2].set_window_cache(false);
    ios
}

/// What a parity run counted: pixels checked against their records, pixels checked covered, and
/// the cached plane run's renders left out and partial.
#[derive(Debug, Default)]
struct Counts {
    images: usize,
    covered: usize,
    skips: u64,
    partial: u64,
}

/// `steps` refreshes of the adversarial edits of seed `seed` on the three runs, compared after
/// each, the plane runs checked against the records.
fn plane_parity(seed: u64, steps: u32) -> Counts {
    let (s0, words) = adv::scene();
    let mut states = [s0.clone(), s0.clone(), s0];
    let mut ios = three_ios();
    let mut rng = X(seed | 1);
    let mut lists = [0, 1, 2].map(|_| Lists {
        base: vec![0; 121],
        lists: vec![Vec::new(); 121],
    });
    let mut n = Counts::default();
    for i in 0..steps {
        let start = rng.clone();
        for k in 0..3 {
            let mut x = start.clone();
            adv::edit(&mut states[k], &ios[k], &mut x, &mut lists[k], &words, i);
            rng = x;
        }
        if rng.pm(5) {
            // A clone of the State takes over (no window memo, the versions kept).
            states = states.map(|s| s.clone());
        }
        if rng.pm(3) {
            ios = three_ios();
        }
        if rng.pm(5) {
            for (s, io) in states.iter().zip(&ios) {
                let mut other = s.clone();
                other.objects.draw.base[60] ^= 1;
                crate::gfx::refresh_view(&mut other, io);
            }
        }
        let before = decisions(&states[1]);
        for (s, io) in states.iter_mut().zip(&ios) {
            crate::gfx::refresh_view(s, io);
        }
        let after = decisions(&states[1]);
        n.skips += after.0 - before.0;
        n.partial += after.1 - before.1;
        let screen = |io: &Io| io.draw(|f| (f.pixels().to_vec(), f.palette));
        let back = |io: &Io| io.peek_back(|b| (b.pixels().to_vec(), b.palette));
        let planes = |io: &Io| {
            let front = io.draw(|f| f.src().map(<[_]>::to_vec));
            (front, io.peek_back(|b| b.src().map(<[_]>::to_vec)))
        };
        let (s0, b0) = (screen(&ios[0]), back(&ios[0]));
        for (k, io) in ios.iter().enumerate().skip(1) {
            assert!(
                screen(io) == s0 && back(io) == b0,
                "seed {seed} step {i}: run {k}'s colours or palettes differ from the run without \
                 the plane"
            );
        }
        assert!(planes(&ios[0]) == (None, None));
        assert!(
            planes(&ios[1]) == planes(&ios[2]),
            "seed {seed} step {i}: the cached plane differs from the every-time render's"
        );
        assert!(
            decisions(&states[0]) == decisions(&states[1]),
            "seed {seed} step {i}: the cache decided otherwise with the plane"
        );
        assert!(
            states[0].rng == states[1].rng && states[1].rng == states[2].rng,
            "seed {seed} step {i}: the RNG"
        );
        assert!(
            states[0] == states[1] && states[1] == states[2],
            "seed {seed} step {i}: the State"
        );
        for k in 1..3 {
            let what = format!("seed {seed} step {i} run {k}");
            let io = &ios[k];
            n.images += io.peek_back(|b| check_images(b, &states[k], &what));
            n.images += io.draw(|f| check_images(f, &states[k], &what));
            n.covered += io.peek_back(|b| check_coverage(b, &states[k], &what));
        }
    }
    n
}

/// Spec 07 Acceptance 1: the plane changes no colour, no palette, no RNG draw, no State and no
/// decision of the window cache over its adversarial edits; the cache leaves the plane as the
/// full render writes it; the plane's attributions are right and cover what the records draw.
/// The cached run took both of its paths.
#[test]
fn the_plane_changes_no_frame_and_follows_the_window_cache() {
    let mut total = Counts::default();
    for seed in 0..2 {
        let n = plane_parity(0x51_7CC1_B727_220A ^ (seed * 0x1000_0001), 400);
        total.images += n.images;
        total.covered += n.covered;
        total.skips += n.skips;
        total.partial += n.partial;
    }
    println!("plane parity: {total:?}");
    assert!(
        total.skips > 0 && total.partial > 0,
        "the cached run took both paths: {total:?}"
    );
    assert!(
        total.images > 0 && total.covered > 0,
        "the checks ran: {total:?}"
    );
}

/// The pointer shape of the paired engines: 3 rows of 16.
fn pointer() -> crate::ui::pointer::PointerState {
    crate::ui::pointer::PointerState::from_tables(crate::ui::pointer::PointerTables {
        shapes: vec![crate::ui::pointer::Shape {
            dx: -2,
            dy: -1,
            rows: vec![[1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0]; 3],
        }],
        shown: true,
        ..Default::default()
    })
}

/// An engine over the adversarial scene: every step its edits and the idle refresh; every 17th
/// a flight (the block under it saved, the sprite drawn and shown, the block put back); every
/// 23rd a glyph and a text scroll over the window on the screen; every 41st a quake and its
/// playback. The plane on or off.
fn paired(seed: u64, plane: bool) -> Engine {
    let (mut s, words) = adv::scene();
    s.ui = crate::main_loop::test_support::test_ui();
    s.ui.pointer = pointer();
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        let mut x = X(seed | 1);
        let mut lists = Lists {
            base: vec![0; 121],
            lists: vec![Vec::new(); 121],
        };
        let font = BitFont {
            glyphs: vec![[0x5A; 8]; 256],
        };
        let text = crate::ui::text::TextWindow {
            x: 3,
            y: 3,
            width: 10,
            height: 10,
            col: 0,
            row: 0,
            fg: 1,
            bg: 2,
            flags: 0,
        };
        for step in 0u32.. {
            {
                let mut s = st.borrow_mut();
                adv::edit(&mut s, &io, &mut x, &mut lists, &words, step);
                crate::gfx::refresh_view(&mut s, &io);
                if step % 17 == 5 {
                    let gfx = &mut s.gfx;
                    let (fx, fy) = (40 + (step % 7) as u16 * 16, 56 + (step % 3) as u16 * 8);
                    crate::gfx::sprite::prepare(gfx, 2, 64 + (step % 18) as u16, 0, 128, 128);
                    io.draw_back(|b| crate::gfx::sprite::save(gfx, b, 3, fx, fy));
                    io.draw_back(|b| crate::gfx::sprite::draw(gfx, b, 2, fx, fy));
                    io.with_buffers(|b, f| crate::gfx::blit::copy_window(b, f));
                    io.draw_back(|b| crate::gfx::sprite::draw(gfx, b, 3, fx, fy));
                }
                if step % 23 == 11 {
                    io.draw(|f| {
                        crate::ui::text::draw_glyph(f, &font, 40, 64, b'x', 1, 2);
                        crate::ui::text::scroll_window(f, &text);
                    });
                }
            }
            if step % 41 == 20 {
                crate::ui::effects::quake(&mut st.borrow_mut(), &io, 2, 3);
                crate::combat::play_effects(&st, &io).await;
            }
            io.present();
            io.vblank().await;
        }
    });
    if plane {
        e.enable_src_plane();
    }
    e
}

/// The colours and palettes of the four buffers: back, screen, published, shown.
fn buffers(e: &Engine) -> [(Vec<u8>, [u6_render::Rgb; 256]); 4] {
    let of = |f: &Framebuffer| (f.pixels().to_vec(), f.palette);
    [
        e.io().peek_back(of),
        e.io().draw(|f| of(f)),
        e.io().published(of),
        of(e.frame()),
    ]
}

/// Paired engines, the plane off and on, the same input (pointer moves every tick): after every
/// tick the four buffers' colours and palettes (the pointer sprite in the shown one), the frame
/// version, the State, the RNG and the cache's decisions are equal; the plane engine's buffers
/// hold only right attributions and its back buffer covers what the records draw.
#[test]
fn paired_engines_show_the_same_with_the_plane_off_and_on() {
    let seed = 0x2545_F491_4F6C_DD1D;
    let (mut off, mut on) = (paired(seed, false), paired(seed, true));
    let mut dice = X(7);
    let (mut images, mut covered) = (0, 0);
    for t in 0..1500 {
        let input = [InputEvent::Pointer {
            x: dice.0 as i16 % 330 - 5,
            y: (dice.0 >> 16) as i16 % 210 - 5,
            buttons: 0,
        }];
        let _ = dice.pm(1);
        off.tick(&input, 20);
        on.tick(&input, 20);
        assert!(
            buffers(&off) == buffers(&on),
            "tick {t}: the buffers' colours or palettes"
        );
        assert_eq!(off.frame_version(), on.frame_version(), "tick {t}");
        let (a, b) = (off.state(), on.state());
        assert!(a.rng == b.rng, "tick {t}: the RNG");
        assert!(
            decisions(&a) == decisions(&b),
            "tick {t}: the cache's decisions"
        );
        assert!(*a == *b, "tick {t}: the State");
        let what = format!("tick {t}");
        images += check_images(on.frame(), &b, &what);
        images += on.io().published(|p| check_images(p, &b, &what));
        images += on.io().draw(|f| check_images(f, &b, &what));
        images += on.io().peek_back(|f| check_images(f, &b, &what));
        covered += on.io().peek_back(|f| check_coverage(f, &b, &what));
    }
    let s = on.state();
    println!(
        "paired engines: 1500 ticks, {images} pixels checked against their records, {covered} \
         covered; {} frames played back, decisions {:?}",
        s.combat.shown,
        decisions(&s)
    );
    assert!(off.frame().src().is_none() && on.frame().src().is_some());
    assert!(s.combat.shown > 0, "quakes were played back");
    assert!(images > 0 && covered > 0);
}

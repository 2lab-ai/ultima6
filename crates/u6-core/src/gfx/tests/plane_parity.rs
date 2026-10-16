//! Spec 07 HD0 parity and an independent check of the plane, on the adversarial edits of the
//! window cache (`adv`): the colours, the palettes, the RNG, the `State` and the cache's
//! decisions are the same with the source plane and without it, over three bare `Io` runs and
//! over two paired engines (every buffer and the pointer sprite, flights, a text scroll, quakes
//! with their playback). In the runs with the plane, every attributed pixel equals its record's
//! pixel at (u, v) when its version is current, and every pixel a record drawn in a window cell
//! writes is attributed: checks that read the records and the window's lists, not the plane's
//! writers. And a quake's recorded frames share equal sources and play back as with their own.
//! No game data.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use u6_formats::font::BitFont;
use u6_formats::gfx::{Span, TileKind};
use u6_render::{Framebuffer, Kind, Src, WIDTH};

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
/// playback, every third of them with a pause after its last picture. The plane on or off; on,
/// after each refresh that presented the window the window rect of the screen's plane must be
/// the back buffer's (counted in the returned cell).
fn paired(seed: u64, plane: bool) -> (Engine, Rc<Cell<u32>>) {
    let (mut s, words) = adv::scene();
    s.ui = crate::main_loop::test_support::test_ui();
    s.ui.pointer = pointer();
    let windows = Rc::new(Cell::new(0));
    let counted = Rc::clone(&windows);
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
                if plane && s.gfx.window_shown != 0 {
                    let rect = |f: &Framebuffer| {
                        let at = (8..168).flat_map(|y| (8..168).map(move |x| (x, y)));
                        at.map(|(x, y)| f.src_at(x, y)).collect::<Vec<_>>()
                    };
                    let (back, front) = (io.peek_back(rect), io.draw(|f| rect(f)));
                    assert!(
                        back == front,
                        "step {step}: the window presented with its sources"
                    );
                    counted.set(counted.get() + 1);
                }
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
                // Every third quake pauses after its last picture, and its last picture is the
                // window put back classic in its own colours (a replay): the playback then ends
                // on the colours it shows and puts back sources other than the frame's in a poll
                // with no present (one would publish them in full), published with the plane
                // alone.
                let pause = step / 41 % 3 == 1;
                {
                    let mut s = st.borrow_mut();
                    crate::ui::effects::quake(&mut s, &io, 2, 3);
                    if pause {
                        crate::combat::present::capture_pause(&mut s, 40);
                        io.draw(|f| {
                            for (x, y) in (0..176).flat_map(|y| (0..176).map(move |x| (x, y))) {
                                let colour = f.get(x, y).expect("on the screen");
                                f.set(x, y, colour);
                            }
                        });
                    }
                }
                crate::combat::play_effects(&st, &io).await;
                if pause {
                    io.vblank().await;
                }
            }
            io.present();
            io.vblank().await;
        }
    });
    if plane {
        e.enable_src_plane();
    }
    (e, windows)
}

/// The pixels the pointer sprite covers in the shown frame.
fn sprite_pixels(e: &Engine) -> BTreeSet<usize> {
    let mut at = BTreeSet::new();
    if let Some(sp) = e.sprite() {
        for (r, row) in sp.rows.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                let (x, y) = (i32::from(sp.x) + c as i32, i32::from(sp.y) + r as i32);
                if v != 0 && (0..WIDTH as i32).contains(&x) && (0..200).contains(&y) {
                    at.insert(y as usize * WIDTH + x as usize);
                }
            }
        }
    }
    at
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
/// hold only right attributions and its back buffer covers what the records draw. And with plain
/// asserts, live without debug assertions: after every tick the published plane is the screen's
/// in full and the shown plane the published one's but for the pointer sprite's pixels, which
/// are classic; after every window present the screen's window rect is the back buffer's. The
/// plane engine published the plane alone at least once (the paused quakes), the moving pointer
/// over it.
#[test]
fn paired_engines_show_the_same_with_the_plane_off_and_on() {
    let seed = 0x2545_F491_4F6C_DD1D;
    let ((mut off, _), (mut on, windows)) = (paired(seed, false), paired(seed, true));
    let mut sprites = 0;
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
        let front = on.io().draw(|f| f.src().unwrap().to_vec());
        let published = on.io().published(|p| p.src().unwrap().to_vec());
        assert!(
            published == front,
            "tick {t}: the published plane is the screen's"
        );
        let sprite = sprite_pixels(&on);
        sprites += usize::from(!sprite.is_empty());
        let shown = on.frame().src().unwrap();
        for (i, (p, s)) in published.iter().zip(shown).enumerate() {
            let want = if sprite.contains(&i) {
                Src::CLASSIC
            } else {
                *p
            };
            assert!(*s == want, "tick {t}: the shown plane at pixel {i}");
        }
    }
    let s = on.state();
    println!(
        "paired engines: 1500 ticks, {images} pixels checked against their records, {covered} \
         covered; {} frames played back, decisions {:?}; {} window presents checked, the sprite \
         shown in {sprites} ticks, {} publishes of the plane alone",
        s.combat.shown,
        decisions(&s),
        windows.get(),
        on.src_publishes()
    );
    assert!(windows.get() > 0 && sprites > 0);
    assert!(off.frame().src().is_none() && on.frame().src().is_some());
    assert!(s.combat.shown > 0, "quakes were played back");
    assert!(images > 0 && covered > 0);
    assert!(
        on.src_publishes() > 0 && off.src_publishes() == 0,
        "the paused quakes published their last sources with the plane alone"
    );
}

/// What [`quake_run`] saw of the frames its quake recorded: per two frames recorded in a row,
/// whether their sources are equal and whether they are one allocation; the allocations of the
/// shakes' sources and of all, and the distinct sources; whether the first frame names a record.
#[derive(Debug, Default)]
struct Shared {
    pairs: Vec<(bool, bool)>,
    shakes: usize,
    all: usize,
    distinct: usize,
    named: bool,
}

/// The adversarial scene shown with the plane, then a quake of 20 shakes of `amp` pixels and its
/// playback. With `unshare`, every recorded frame is given sources of its own before the
/// playback (as without the sharing). Returns the engine and what the quake recorded.
fn quake_run(amp: i16, unshare: bool) -> (Engine, Rc<std::cell::RefCell<Shared>>) {
    let (mut s, words) = adv::scene();
    s.ui = crate::main_loop::test_support::test_ui();
    let seen = Rc::new(std::cell::RefCell::new(Shared::default()));
    let out = Rc::clone(&seen);
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        {
            let mut s = st.borrow_mut();
            let mut lists = Lists {
                base: vec![0; 121],
                lists: vec![Vec::new(); 121],
            };
            adv::edit(&mut s, &io, &mut X(0x9E37_79B9), &mut lists, &words, 0);
            crate::gfx::refresh_view(&mut s, &io);
        }
        io.present();
        io.vblank().await;
        {
            let mut s = st.borrow_mut();
            crate::ui::effects::quake(&mut s, &io, 20, amp);
            let shots: Vec<Rc<[Src]>> = s
                .combat
                .effects
                .iter()
                .filter_map(|e| e.src.0.clone())
                .collect();
            let allocations = |shots: &[Rc<[Src]>]| {
                let at = shots.iter().map(|r| Rc::as_ptr(r).cast::<Src>() as usize);
                at.collect::<BTreeSet<_>>().len()
            };
            let mut o = out.borrow_mut();
            o.pairs = shots
                .windows(2)
                .map(|p| (p[0][..] == p[1][..], Rc::ptr_eq(&p[0], &p[1])))
                .collect();
            o.shakes = allocations(&shots[..20]);
            o.all = allocations(&shots);
            o.distinct = (0..shots.len())
                .filter(|&i| shots[..i].iter().all(|b| b[..] != shots[i][..]))
                .count();
            o.named = shots[0].iter().any(|src| src.kind == Kind::Tile);
            if unshare {
                for e in &mut s.combat.effects {
                    if let Some(v) = &e.src.0 {
                        e.src.0 = Some(Rc::from(&v[..]));
                    }
                }
            }
        }
        crate::combat::play_effects(&st, &io).await;
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    (e, seen)
}

/// The four planes: back, screen, published, shown.
fn planes(e: &Engine) -> [Option<Vec<Src>>; 4] {
    let of = |f: &Framebuffer| f.src().map(<[Src]>::to_vec);
    [
        e.io().peek_back(of),
        e.io().draw(|f| of(f)),
        e.io().published(of),
        of(e.frame()),
    ]
}

/// A quake's recorded frames whose sources equal the frame's before hold one allocation, the
/// others their own: a quake of 0 pixels (every shake the same window) shares one between its 20
/// shakes, one of 3 pixels shares only where a shake lands where the one before it did. The
/// playback shows the same with the sharing as with every frame's sources its own: after every
/// tick the colours, palettes and planes of the four buffers, the frame and source versions and
/// the State.
#[test]
fn a_quakes_equal_frames_share_their_sources_and_play_back_the_same() {
    let mut pairs = Vec::new();
    for amp in [0, 3] {
        let ((mut shared, seen), (mut own, _)) = (quake_run(amp, false), quake_run(amp, true));
        for t in 0..200 {
            shared.tick(&[], 20);
            own.tick(&[], 20);
            let what = format!("amplitude {amp} tick {t}");
            assert!(buffers(&shared) == buffers(&own), "{what}: the colours");
            assert!(planes(&shared) == planes(&own), "{what}: the planes");
            assert_eq!(
                (shared.frame_version(), shared.src_version()),
                (own.frame_version(), own.src_version()),
                "{what}: the versions"
            );
            assert!(*shared.state() == *own.state(), "{what}: the State");
        }
        let seen = seen.borrow();
        println!(
            "quake of {amp} pixels: {} frames recorded, their sources in {} allocations ({} for \
             the 20 shakes) and {} distinct, {} of {} pairs in a row equal",
            seen.pairs.len() + 1,
            seen.all,
            seen.shakes,
            seen.distinct,
            seen.pairs.iter().filter(|p| p.0).count(),
            seen.pairs.len()
        );
        assert_eq!(seen.pairs.len(), 20, "20 shakes and the window");
        assert!(seen.named, "the frames name records");
        assert_eq!(shared.state().combat.shown, 21, "all played back");
        assert!(shared.state().combat.effects.is_empty());
        if amp == 0 {
            assert_eq!(seen.shakes, 1, "the shakes of 0 pixels hold one allocation");
        }
        pairs.extend_from_slice(&seen.pairs);
    }
    for (k, &(equal, one)) in pairs.iter().enumerate() {
        assert_eq!(equal, one, "pair {k}: one allocation if and only if equal");
    }
    assert!(pairs.iter().any(|p| p.0) && pairs.iter().any(|p| !p.0));
}

//! Real data (spec 20 receipts): smooth motion is presentation only. The game booted as the
//! oracle's game-first disks start it (the start fight), then passes and party steps by arrow key;
//! one engine with the switch on and one with it off, in lockstep. Skips without the user's zip
//! (`$HOME/Games/ultima6/original/ultima-vi.zip`). Prints counts the test made only (spec 01
//! §Rights).
#![allow(clippy::disallowed_methods)] // the test reads the zip

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use u6_intake::{InputFile, intake};
use u6_render::motion::{Presenter, SLIDE_MS, View};
use u6_render::{Framebuffer, Src};
use u6_testkit::Quiet;

use super::motion::{Shot, paint, slide_layers, slide_placements};
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::geom::SHOWN;
use crate::hd::HdView;
use crate::pacer::STEP_MS;
use crate::state::State;
use u6_render::hd::Scale;

fn files() -> Option<Files> {
    let zip = PathBuf::from(std::env::var_os("HOME")?).join("Games/ultima6/original/ultima-vi.zip");
    if !zip.exists() {
        println!("skipped: no game files (the user's zip is not present)");
        return None;
    }
    let data = std::fs::read(&zip).quiet();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// The turns of the fight: three passes through the start fight, then steps round the castle, the
/// fight going on (combat mode on).
const TURNS: [Key; 14] = [
    Key::Space,
    Key::Space,
    Key::Space,
    Key::Up,
    Key::Up,
    Key::Right,
    Key::Right,
    Key::Down,
    Key::Left,
    Key::Left,
    Key::Up,
    Key::Right,
    Key::Down,
    Key::Down,
];
/// The steps of the town: round the castle with no fight (the oracle's town setup, [`play`]).
const STEPS: [Key; 10] = [
    Key::Up,
    Key::Up,
    Key::Right,
    Key::Right,
    Key::Down,
    Key::Down,
    Key::Left,
    Key::Left,
    Key::Up,
    Key::Down,
];
/// Ticks a turn may take at most.
const TURN_TICKS: usize = 4000;

/// The page's engine: `game` at once with `seed` (as [`Engine::game_first`]), the source plane and
/// the remembered sight on (the page's defaults), smooth motion as `motion`; and its state.
fn boot(files: &Files, seed: u16, motion: bool) -> (Engine, Rc<RefCell<State>>) {
    let mut state = State::new_game(files).quiet();
    state.rng.seed(seed);
    let keep = Rc::new(RefCell::new(None));
    let k = Rc::clone(&keep);
    let mut e = Engine::with_game(state, files.clone(), move |st, io| {
        *k.borrow_mut() = Some(Rc::clone(&st));
        crate::main_loop::game_first(st, io)
    });
    e.enable_src_plane();
    e.io().set_sight_memory(true);
    e.set_motion(motion);
    e.tick(&[], 0);
    let state = keep.borrow().clone().quiet();
    (e, state)
}

fn at_prompt(e: &Engine) -> bool {
    e.io().pending_events() == 0 && e.state().main.reading == 1
}

/// The published frame's colours, sources and palette, as one hash.
fn publication(e: &Engine) -> u64 {
    use std::hash::{Hash, Hasher};
    let f = e.published_frame();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.pixels().hash(&mut h);
    f.src().hash(&mut h);
    for c in f.palette.iter() {
        (c.r, c.g, c.b).hash(&mut h);
    }
    h.finish()
}

/// Runs the boot, then with `town` the oracle's town setup (tools/oracle/game.mjs townSetup: the
/// actors in the assault worktype 8 passed over, status bit 0x10, and combat broken off with `b`)
/// and [`STEPS`], else [`TURNS`]; one tick of [`STEP_MS`] at a time, calling `each` after every
/// tick.
fn play(
    (e, state): &mut (Engine, Rc<RefCell<State>>),
    town: bool,
    mut each: impl FnMut(&mut Engine),
) {
    let mut settle = |e: &mut Engine, input: &[InputEvent]| {
        e.tick(input, STEP_MS);
        each(e);
        for _ in 0..TURN_TICKS {
            if at_prompt(e) {
                return;
            }
            e.tick(&[], STEP_MS);
            each(e);
        }
        panic!("a turn did not come back to the prompt");
    };
    settle(e, &[]);
    if !town {
        for k in TURNS {
            settle(e, &[InputEvent::Key(k)]);
        }
        return;
    }
    {
        let mut s = state.borrow_mut();
        for a in 0..0x100 {
            if s.actors.worktype[a] == 8 {
                s.actors.attrs.status[a] |= 0x10;
            }
        }
    }
    settle(e, &[InputEvent::Key(Key::Char('b'))]);
    for k in STEPS {
        settle(e, &[InputEvent::Key(k)]);
    }
}

#[test]
fn smooth_motion_leaves_the_state_the_rng_and_the_publications_as_without_it() {
    let Some(files) = files() else { return };
    for (seed, town) in [0, 7, 100]
        .into_iter()
        .flat_map(|s| [(s, false), (s, true)])
    {
        let mut on = boot(&files, seed, true);
        let mut off = boot(&files, seed, false);
        let mut streams = [Vec::new(), Vec::new()];
        let mut scenes = [Vec::new(), Vec::new()];
        for (k, e) in [&mut on, &mut off].into_iter().enumerate() {
            let mut seen = 0;
            let (stream, kept) = (&mut streams[k], &mut scenes[k]);
            play(e, town, |e| {
                if e.published_version() != seen {
                    seen = e.published_version();
                    stream.push((e.ticks(), publication(e)));
                }
                kept.extend(e.take_motion());
            });
        }
        let shown = scenes[0].iter().filter(|p| p.shot.is_some()).count();
        let early = scenes[0].iter().filter(|p| p.preview).count();
        let run = if town { "town" } else { "fight" };
        assert!(shown > 0, "seed {seed} {run}: the switch on keeps scenes");
        assert!(scenes[1].is_empty(), "seed {seed} {run}: off keeps none");
        u6_testkit::assert_same!(streams[0], streams[1], "the publications");
        let (on, off) = (on.0.state(), off.0.state());
        assert!(
            *on == *off,
            "seed {seed} {run}: the state, its RNG included"
        );
        assert_eq!(on.rng, off.rng, "seed {seed} {run}: the RNG");
        println!(
            "seed {seed} {run}: {} publications equal; with the switch on {} scenes ({early} \
             early pictures), {} foreign; state equal",
            streams[0].len(),
            shown - early,
            scenes[0].len() - shown
        );
    }
}

/// The poison the shown map is filled with before a slide is drawn over it: a colour and a source
/// no draw writes there.
const POISON: (u8, Src) = (
    0xab,
    Src {
        ver: u32::MAX,
        id: u16::MAX,
        u: 0,
        v: 0,
        kind: u6_render::Kind::Glyph,
        flags: 0,
    },
);
/// The HD word of the poison (no layer's: its alpha is 0 and its colour not black).
const POISON_WORD: u32 = 0x00ab_cdef;

/// The published frame of `e` with its shown map poisoned ([`POISON`]).
fn poisoned(e: &Engine) -> Framebuffer {
    let mut fb = e.published_frame().clone();
    let (x0, y0, x1, y1) = SHOWN;
    for y in usize::from(y0)..=usize::from(y1) {
        for x in usize::from(x0)..=usize::from(x1) {
            fb.set_src(x, y, POISON.0, POISON.1);
        }
    }
    fb
}

/// Whether frame pixel (x, y) lies in the shown map.
fn in_shown(x: usize, y: usize) -> bool {
    let (x0, y0, x1, y1) = SHOWN;
    (usize::from(x0)..=usize::from(x1)).contains(&x)
        && (usize::from(y0)..=usize::from(y1)).contains(&y)
}

/// `a` against `b`: over the shown map how many pixels differ in colour and in source, and outside
/// it how many differ in either.
fn differ(a: &Framebuffer, b: &Framebuffer) -> (usize, usize, usize) {
    let (mut colours, mut sources, mut outside) = (0, 0, 0);
    for y in 0..u6_render::HEIGHT {
        for x in 0..u6_render::WIDTH {
            let src = |f: &Framebuffer| f.src_at(x, y).unwrap_or(Src::CLASSIC);
            let (c, s) = (a.get(x, y) != b.get(x, y), src(a) != src(b));
            if in_shown(x, y) {
                colours += usize::from(c);
                sources += usize::from(s);
            } else {
                outside += usize::from(c || s);
            }
        }
    }
    (colours, sources, outside)
}

/// How a receipt draws a slide ([`classic`], [`hd_drawn`]): as the port does, or broken (a
/// negative control: the receipt must fail).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Draws {
    Port,
    /// The paint draws nothing (classic); no layer is drawn (HD).
    Nothing,
}

/// Classic: [`paint`] of `p`'s plan for `view` over the published frame with the shown map
/// poisoned, against `want`: [`differ`].
fn classic(
    e: &Engine,
    p: &Presenter<Shot>,
    view: &View,
    draws: Draws,
    want: &Framebuffer,
) -> (usize, usize, usize) {
    let mut fb = poisoned(e);
    if draws == Draws::Port {
        paint(&e.state(), p, view, &mut fb);
    }
    differ(&fb, want)
}

/// HD (spec 20 Design 5): the HD frame with the shown map poisoned, the slide's layers
/// ([`slide_layers`], each composed by `hd`) drawn over it where [`slide_placements`] puts them for
/// `view`, clipped to the shown map, each layer's clear words left out; `None` without layers.
fn hd_drawn(
    e: &Engine,
    hd: &mut HdView,
    p: &Presenter<Shot>,
    view: &View,
    draws: Draws,
) -> Option<Vec<u32>> {
    let s = hd.scale().factor();
    let ow = u6_render::WIDTH * s;
    let mut out = hd.rgba().to_vec();
    for (k, w) in out.iter_mut().enumerate() {
        if in_shown(k % ow / s, k / ow / s) {
            *w = POISON_WORD;
        }
    }
    let slide = slide_layers(&e.state(), p)?;
    let mut words = Vec::new();
    for (i, x, y) in slide_placements(&slide, view) {
        if draws == Draws::Nothing {
            break;
        }
        let px = &slide.parts[i].pixels;
        hd.compose_layer(e, px, &mut words);
        let w = px.width * s;
        for (k, &word) in words.iter().enumerate() {
            let (dx, dy) = (x * s as i32 + (k % w) as i32, y * s as i32 + (k / w) as i32);
            if word >> 24 != 0 && dx >= 0 && dy >= 0 && in_shown(dx as usize / s, dy as usize / s) {
                out[dy as usize * ow + dx as usize] = word;
            }
        }
    }
    Some(out)
}

/// The words of `drawn` that differ from the HD frame, and the poisoned ones left (shown words no
/// layer covers); both all of the shown map without layers.
fn hd_differ(hd: &HdView, drawn: Option<&[u32]>) -> (usize, usize) {
    let Some(drawn) = drawn else {
        let n = hd.rgba().len();
        return (n, n);
    };
    let differ = (drawn.iter().zip(hd.rgba()))
        .filter(|(a, b)| a != b)
        .count();
    (differ, drawn.iter().filter(|&&w| w == POISON_WORD).count())
}

/// Spec 20 Design 3 and 5, on the user's zip: at the end of every slide (the turns of the fight and
/// the town, seeds 0, 7 and 100) the map drawn again from scratch (over a shown map poisoned) is
/// the engine's picture: classic ([`paint`]) in colour and source with nothing outside the shown
/// map changed, and HD (the layers at 2× over the HD frame) word for word with every shown word
/// covered by a layer; half way through each slide nothing outside the shown map changes (classic)
/// and every shown word is drawn by a layer (HD). The two controls, a paint that draws nothing and
/// no layers drawn, fail on every scene.
#[test]
fn a_slide_at_its_end_draws_the_engines_picture_of_every_scene() {
    let Some(files) = files() else { return };
    let mut totals = [0usize; 6];
    for (seed, town) in [0, 7, 100]
        .into_iter()
        .flat_map(|s| [(s, false), (s, true)])
    {
        let mut e = boot(&files, seed, true);
        let mut p: Presenter<Shot> = Presenter::new();
        let mut checked = [0usize; 6];
        let mut bad = Vec::new();
        let mut hd = HdView::new();
        hd.set(true, Scale::X2);
        play(&mut e, town, |e| {
            for publ in e.take_motion() {
                let Some(shot) = publ.shot else {
                    p.foreign();
                    checked[4] += 1;
                    continue;
                };
                let time = publ.time_ms as f64;
                if publ.preview {
                    p.preview(shot, time, true);
                } else {
                    p.publish(shot, time, true);
                }
                let start = p.view(time);
                checked[1] += usize::from(start.as_ref().is_some_and(|v| !v.movers.is_empty()));
                checked[2] += usize::from(start.is_some_and(|v| v.camera != (0, 0)));
                if publ.preview {
                    continue;
                }
                let view = p.view(time + SLIDE_MS).unwrap_or(View {
                    camera: (0, 0),
                    movers: Vec::new(),
                });
                checked[0] += 1;
                let want = e.published_frame().clone();
                let (colours, sources, outside) = classic(e, &p, &view, Draws::Port, &want);
                hd.present(Some(e), &Framebuffer::new());
                let drawn = hd_drawn(e, &mut hd, &p, &view, Draws::Port);
                let (words, uncovered) = hd_differ(&hd, drawn.as_deref());
                if colours + sources + outside + words + uncovered > 0 {
                    bad.push((e.ticks(), colours, sources, outside, words, uncovered));
                }
                // The two controls: a paint that draws nothing and no layers drawn must fail the
                // checks above. (Without layers made the HD check fails by its definition: a
                // sanity check of `hd_differ`, no control.)
                let (c, _, _) = classic(e, &p, &view, Draws::Nothing, &want);
                let none = hd_drawn(e, &mut hd, &p, &view, Draws::Nothing);
                let (w, u) = hd_differ(&hd, none.as_deref());
                assert!(
                    c > 0 && w > 0 && u > 0,
                    "tick {}: the controls fail",
                    e.ticks()
                );
                assert!(
                    hd_differ(&hd, None).1 > 0,
                    "sanity: no layers, nothing covered"
                );
                checked[5] += 1;
                // Half way: classic, nothing outside the shown map changed (the paint clears the
                // whole shown map first, so no pixel of it is left as it was); HD, every shown
                // word drawn by a layer.
                if let Some(mid) = p.view(time + SLIDE_MS / 2.0) {
                    let mut fb = poisoned(e);
                    paint(&e.state(), &p, &mid, &mut fb);
                    let (_, _, outside) = differ(&fb, &want);
                    let words = hd_drawn(e, &mut hd, &p, &mid, Draws::Port);
                    let (_, uncovered) = hd_differ(&hd, words.as_deref());
                    if outside + uncovered > 0 {
                        bad.push((e.ticks(), 0, 0, outside, 0, uncovered));
                    }
                    checked[3] += usize::from(!mid.movers.is_empty());
                }
            }
        });
        assert!(
            bad.is_empty(),
            "seed {seed} town {town}: (tick, colours, sources, outside, HD words, HD words \
             uncovered) differing: {bad:?}"
        );
        assert!(
            checked[1] > 0 && checked[2] > 0,
            "seed {seed} town {town}: movers and camera slides seen"
        );
        for (t, c) in totals.iter_mut().zip(checked) {
            *t += c;
        }
    }
    println!(
        "{} scenes drawn again at a slide's end over a poisoned map, equal to the engine's picture \
         (classic, and the HD layers at 2x covering every shown word), each with its 2 controls \
         failing ({}); slides (from early pictures and scenes) with movers {}, with the camera {}; \
         {} scenes drawn half way with movers, nothing outside the map changed (classic) and \
         every shown word drawn (HD); {} foreign publications",
        totals[0], totals[5], totals[1], totals[2], totals[3], totals[4]
    );
}

/// Spec 20 Design 4: each step that moved the leader out of combat mode hands over its early
/// picture in the tick it was taken, ahead of the engine's own picture of the turn, with no foreign
/// picture between them (which would show the party back where it was: a fight's missile, seen in
/// Wide at seed 7 before combat mode was left out), at least a tick ahead and within the turn, and
/// that picture has the leader and every follower where the early one had them, so their slides
/// run on; none is left without its turn's picture; in combat mode none is made.
#[test]
fn the_early_picture_comes_with_the_step_and_has_the_party_where_the_turns_picture_has_it() {
    let Some(files) = files() else { return };
    let (mut previews, mut ahead, mut followers) = (0, Vec::new(), [0usize; 2]);
    let mut in_combat = 0;
    for (seed, town) in [0, 7, 100]
        .into_iter()
        .flat_map(|s| [(s, false), (s, true)])
    {
        let mut e = boot(&files, seed, true);
        let mut held: Option<(Rc<Shot>, u64)> = None;
        let mut bad = Vec::new();
        let mut cut = Vec::new();
        play(&mut e, town, |e| {
            for p in e.take_motion() {
                let Some(shot) = p.shot else {
                    if held.take().is_some() {
                        cut.push((e.ticks(), e.state().actors.combat_mode()));
                    }
                    continue;
                };
                if p.preview {
                    previews += 1;
                    in_combat += usize::from(e.state().actors.combat_mode());
                    held = Some((shot, e.ticks()));
                    continue;
                }
                let Some((early, at)) = held.take() else {
                    continue;
                };
                ahead.push(e.ticks() - at);
                let (a, b) = (&early.scene, &shot.scene);
                let leader = a.leader.and_then(|l| a.find(l)).map(|p| p.cell);
                if leader != b.leader.and_then(|l| b.find(l)).map(|p| p.cell) {
                    bad.push(e.ticks());
                }
                let party: Vec<u16> = e.state().actors.party.members
                    [..e.state().actors.party.count()]
                    .iter()
                    .map(|&m| u16::from(m))
                    .collect();
                for m in party.into_iter().filter(|&m| Some(m) != a.leader) {
                    if let (Some(x), Some(y)) = (a.find(m), b.find(m)) {
                        followers[usize::from(x.cell == y.cell)] += 1;
                    }
                }
            }
        });
        assert!(
            bad.is_empty(),
            "seed {seed}: the leader elsewhere at ticks {bad:?}"
        );
        assert!(
            cut.is_empty(),
            "seed {seed}: early pictures cut by a foreign picture (tick, combat): {cut:?}"
        );
        assert!(
            held.is_none(),
            "seed {seed}: the last early picture met its turn's picture"
        );
    }
    assert!(previews > 0, "early pictures made");
    assert_eq!(
        ahead.len(),
        previews,
        "each early picture met its turn's picture"
    );
    assert!(
        ahead.iter().all(|&t| (1..TURN_TICKS as u64).contains(&t)),
        "each ahead of its turn's picture, within the turn: {ahead:?}"
    );
    assert!(
        followers[1] > 0 && followers[0] == 0,
        "the followers where the early picture had them: {followers:?}"
    );
    assert_eq!(in_combat, 0, "none in combat mode");
    ahead.sort_unstable();
    println!(
        "{previews} early pictures; the turn's picture came {}..{} ticks after (median {}); \
         followers where the early picture had them {}, moved since {}",
        ahead[0],
        ahead[ahead.len() - 1],
        ahead[ahead.len() / 2],
        followers[1],
        followers[0]
    );
}

/// Timings only (run with `--ignored --nocapture`, gate profile): spec 20 Design 4's early
/// picture clones the whole `State` once a step, budget 1 ms at 8× with the held walk. The
/// clone's wall time, then the whole early picture's, over 200 of each on the state after the
/// start fight and the steps.
#[test]
#[ignore]
#[allow(clippy::disallowed_types)] // a timing: wall time, outside the engine
fn bench_the_state_clone_of_the_early_picture() {
    let Some(files) = files() else { return };
    let mut e = boot(&files, 0, false);
    play(&mut e, true, |_| {});
    let (e, _) = e;
    let state = e.state().clone();
    let mut times: Vec<f64> = (0..200)
        .map(|_| {
            let t = std::time::Instant::now();
            let copy = std::hint::black_box(state.clone());
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            drop(copy);
            ms
        })
        .collect();
    times.sort_by(f64::total_cmp);
    println!(
        "State clone: median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
        times[100], times[190], times[199]
    );
    // The whole early picture: the clone, the view and its draw lists on it, the scene read.
    e.io().set_motion(true);
    let mut times: Vec<f64> = (0..200)
        .map(|_| {
            let t = std::time::Instant::now();
            super::motion::preview(&state, e.io());
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    println!(
        "early picture (clone, view, draw lists, scene): median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
        times[100], times[190], times[199]
    );
}

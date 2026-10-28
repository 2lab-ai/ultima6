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

/// The user's zip, or none: the test skips, saying so, unless `U6_REQUIRE_ARCHIVE=1` (the release
/// receipt), which fails it instead. With it the test says it ran.
fn files() -> Option<Files> {
    let zip = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Games/ultima6/original/ultima-vi.zip"))
        .filter(|zip| zip.exists());
    let Some(zip) = zip else {
        assert_ne!(
            std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
            Ok("1"),
            "U6_REQUIRE_ARCHIVE=1 and the user's zip is not present"
        );
        println!("skipped: no game files (the user's zip is not present)");
        return None;
    };
    println!("ran: on the user's zip");
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
    turn(e, &[], &mut each);
    if !town {
        for k in TURNS {
            turn(e, &[InputEvent::Key(k)], &mut each);
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
    turn(e, &[InputEvent::Key(Key::Char('b'))], &mut each);
    for k in STEPS {
        turn(e, &[InputEvent::Key(k)], &mut each);
    }
}

/// One turn: `input`, then ticks of [`STEP_MS`] until the prompt is back (at most
/// [`TURN_TICKS`]), calling `each` after every tick.
fn turn(e: &mut Engine, input: &[InputEvent], each: &mut impl FnMut(&mut Engine)) {
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
    // Per run kind (the fight, the town): the slides' generations, the leader's steps.
    let (mut generations, mut steps) = ([0usize; 2], [0usize; 2]);
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
        // The generations of the slides, and the leader's steps (scenes that moved its cell).
        let (mut generation, mut leader) = (p.generation(), None);
        play(&mut e, town, |e| {
            for publ in e.take_motion() {
                let Some(shot) = publ.shot else {
                    p.foreign();
                    checked[4] += 1;
                    continue;
                };
                let time = publ.time_ms as f64;
                let at = shot
                    .scene
                    .leader
                    .and_then(|l| shot.scene.find(l))
                    .map(|l| l.cell);
                if at != leader && !publ.preview {
                    steps[usize::from(town)] += usize::from(leader.is_some());
                    leader = at;
                }
                if publ.preview {
                    p.preview(shot, time, true);
                } else {
                    p.publish(shot, time, true);
                }
                if p.generation() != generation {
                    generation = p.generation();
                    generations[usize::from(town)] += 1;
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
    println!(
        "the slides' generations against the leader's steps: the fight {} in {} steps, the town \
         {} in {} steps",
        generations[0], steps[0], generations[1], steps[1]
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

/// Remembers every cell within 12 of the leader's on its level as last seen `days` whole game days
/// ago, its look the leader's cell's ground ([`super::cache::SightMemory::remember`]): a cell in
/// sight at the next framed render takes its own look again, one out of sight shows this one,
/// faded `days`.
fn remember_round(state: &mut State, days: u8) {
    let now = super::cache::minutes(&state.actors.calendar);
    let c = state.map.centre;
    let i = crate::geom::HALF_H * crate::geom::VIEW_W + crate::geom::HALF_W;
    let t = usize::from(state.objects.draw.base[i] & 0x7ff);
    let gfx = &mut state.gfx;
    let layer = super::cache::Layer {
        record: gfx.record_of[t],
        masktype: gfx.masktype[t],
        frame: 0,
        frozen: None,
    };
    let seen = now - u64::from(days) * 24 * 60;
    for dy in -12i32..=12 {
        for dx in -12i32..=12 {
            let (x, y) = ((i32::from(c.x) + dx) as u16, (i32::from(c.y) + dy) as u16);
            gfx.cache.memory.remember((c.z, x, y), vec![layer], seen);
        }
    }
}

/// Whether pixel `(x, y)` of `fb` is drawn from the remembered sight ([`u6_render::FLAG_MEMORY`]).
fn remembered(fb: &Framebuffer, x: usize, y: usize) -> bool {
    fb.src_at(x, y)
        .is_some_and(|s| s.flags & u6_render::FLAG_MEMORY != 0)
}

/// Spec 18 and 20 Design 3–5, on the user's zip: remembered cells seen 0 to 4 game days ago (after
/// the town's steps, the cells round the leader remembered that long ago: [`remember_round`];
/// then the steps again). At the end of every slide
/// the map drawn again over a poisoned map is the engine's picture in colour and source, classic
/// and HD, the faded cells included; and each early picture (out of combat) drawn at its start
/// shows every cell out of sight in it and in the engine's picture then as that picture shows it,
/// pixel for pixel (its black pixels and the dither's holes included), colour and source.
#[test]
fn remembered_cells_slide_with_their_fade_and_the_early_picture_shows_them() {
    let Some(files) = files() else { return };
    let mut totals = [0usize; 4];
    for days in 0..=4u8 {
        let mut game = boot(&files, 0, true);
        play(&mut game, true, |e| drop(e.take_motion()));
        remember_round(&mut game.1.borrow_mut(), days);
        let (mut e, _state) = game;
        let mut p: Presenter<Shot> = Presenter::new();
        let mut hd = HdView::new();
        hd.set(true, Scale::X2);
        let (mut bad, mut early_bad) = (Vec::new(), Vec::new());
        let mut seen = [0usize; 4];
        let mut each = |e: &mut Engine| {
            for publ in e.take_motion() {
                let Some(shot) = publ.shot else {
                    p.foreign();
                    continue;
                };
                let time = publ.time_ms as f64;
                let want = e.published_frame().clone();
                if publ.preview {
                    // The early picture at its start, against the engine's picture now.
                    p.preview(Rc::clone(&shot), time, true);
                    let Some(start) = p.view(time) else { continue };
                    let mut fb = poisoned(e);
                    paint(&e.state(), &p, &start, &mut fb);
                    let n = &shot.scene;
                    // The engine's scene shown now (the picture before the step).
                    let before = p.history().get(1).map(|h| &h.scene);
                    let unseen_in = |s: &u6_render::motion::Scene, (rx, ry): (i32, i32)| {
                        let cell = (
                            s.origin.0 + (rx - s.corner.0).div_euclid(16),
                            s.origin.1 + (ry - s.corner.1).div_euclid(16),
                        );
                        s.index(cell).is_some_and(|i| {
                            let base = s.cells[i].base;
                            base == crate::map::view::NOT_VISIBLE || base == crate::map::view::DARK
                        })
                    };
                    let (mut compared, mut differ) = (0, 0);
                    let (x0, y0, x1, y1) = SHOWN;
                    for y in usize::from(y0)..=usize::from(y1) {
                        for x in usize::from(x0)..=usize::from(x1) {
                            // A cell out of sight in the early picture (drawn at (x, y) at its
                            // start) and in the engine's picture: every pixel of it, its black ones
                            // and the dither's holes included, colour and source.
                            let start_at = (x as i32 + start.camera.0, y as i32 + start.camera.1);
                            let rest = (x as i32, y as i32);
                            if unseen_in(n, start_at) && before.is_some_and(|b| unseen_in(b, rest))
                            {
                                compared += 1;
                                let same = fb.get(x, y) == want.get(x, y)
                                    && fb.src_at(x, y) == want.src_at(x, y);
                                differ += usize::from(!same);
                            }
                        }
                    }
                    seen[2] += 1;
                    seen[3] += compared;
                    if differ > 0 {
                        early_bad.push((e.ticks(), compared, differ));
                    }
                    continue;
                }
                p.publish(shot, time, true);
                let view = p.view(time + SLIDE_MS).unwrap_or(View {
                    camera: (0, 0),
                    movers: Vec::new(),
                });
                let (colours, sources, outside) = classic(e, &p, &view, Draws::Port, &want);
                hd.present(Some(e), &Framebuffer::new());
                let drawn = hd_drawn(e, &mut hd, &p, &view, Draws::Port);
                let (words, uncovered) = hd_differ(&hd, drawn.as_deref());
                if colours + sources + outside + words + uncovered > 0 {
                    bad.push((e.ticks(), colours, sources, outside, words, uncovered));
                }
                let (x0, y0, x1, y1) = SHOWN;
                let memory = (usize::from(y0)..=usize::from(y1))
                    .flat_map(|y| (usize::from(x0)..=usize::from(x1)).map(move |x| (x, y)))
                    .filter(|&(x, y)| remembered(&want, x, y))
                    .count();
                seen[0] += 1;
                seen[1] += memory;
            }
        };
        for k in STEPS {
            turn(&mut e, &[InputEvent::Key(k)], &mut each);
        }
        assert!(
            bad.is_empty(),
            "{days} days: (tick, colours, sources, outside, HD words, HD words uncovered) \
             differing at a slide's end: {bad:?}"
        );
        assert!(
            early_bad.is_empty(),
            "{days} days: (tick, remembered pixels compared, differing) in early pictures: \
             {early_bad:?}"
        );
        assert!(
            seen[1] > 0 && seen[2] > 0 && seen[3] > 0,
            "{days} days: remembered pixels at slides' ends and in early pictures: {seen:?}"
        );
        println!(
            "{days} days: {} slide ends equal to the engine's picture, {} remembered pixels in \
             them; {} early pictures, {} pixels of cells out of sight each as the engine's picture",
            seen[0], seen[1], seen[2], seen[3]
        );
        for (t, s) in totals.iter_mut().zip(seen) {
            *t += s;
        }
    }
    println!("in all: {totals:?}");
}

/// Whether a slide runs at `t` (a camera or a mover off its place; a preview held at rest runs
/// none).
fn slides_at(p: &Presenter<Shot>, t: f64) -> bool {
    p.view(t)
        .is_some_and(|v| v.camera != (0, 0) || !v.movers.is_empty())
}

/// The town's setup of [`play`]: the actors in the assault worktype passed over (status bit
/// 0x10).
fn pass_assault(s: &mut State) {
    for a in 0..0x100 {
        if s.actors.worktype[a] == 8 {
            s.actors.attrs.status[a] |= 0x10;
        }
    }
}

/// Spec 20 (load state 0x105350, spec 18's forget): no slide crosses a load. In the town, out of
/// combat: the party steps a cell, the game is restored (the boot's save: the party back a cell
/// away, the remembered sight forgotten; combat broken off again in the state), then the first
/// input is a step across the first one's way. The presenter holds a scene published before the
/// load when it comes. The first publication after the load (the step's early picture) snaps,
/// and after each publication after the load the presenter holds none of the scenes published
/// before it (by identity): nothing slides from them or draws from their store.
#[test]
fn no_slide_crosses_a_load() {
    let Some(files) = files() else { return };
    let (mut e, state) = boot(&files, 0, true);
    let p: RefCell<Presenter<Shot>> = RefCell::new(Presenter::new());
    // The scenes published since it was emptied: whether early, snapped, and slid into.
    let seen: RefCell<Vec<(bool, bool, bool)>> = RefCell::new(Vec::new());
    // The scenes published before the load (held, so no later scene takes their place), and
    // whether it came.
    let before: RefCell<Vec<Rc<Shot>>> = RefCell::new(Vec::new());
    let loaded = std::cell::Cell::new(false);
    let held_before = |p: &Presenter<Shot>| {
        (p.history().iter())
            .filter(|h| before.borrow().iter().any(|b| Rc::ptr_eq(h, b)))
            .count()
    };
    let checked = std::cell::Cell::new(0usize);
    let mut feed = |e: &mut Engine| {
        for publ in e.take_motion() {
            let time = publ.time_ms as f64;
            let mut p = p.borrow_mut();
            match publ.shot {
                None => p.foreign(),
                Some(shot) => {
                    if !loaded.get() {
                        before.borrow_mut().push(Rc::clone(&shot));
                    }
                    let snap = shot.scene.snap;
                    if publ.preview {
                        p.preview(shot, time, true);
                    } else {
                        p.publish(shot, time, true);
                    }
                    seen.borrow_mut()
                        .push((publ.preview, snap, slides_at(&p, time)));
                }
            }
            if loaded.get() {
                assert_eq!(
                    held_before(&p),
                    0,
                    "a scene from before the load held after a publication after it"
                );
                checked.set(checked.get() + 1);
            }
        }
    };
    turn(&mut e, &[], &mut feed);
    pass_assault(&mut state.borrow_mut());
    turn(&mut e, &[InputEvent::Key(Key::Char('b'))], &mut feed);
    let at = |s: &RefCell<State>| (s.borrow().map.centre.x, s.borrow().map.centre.y);
    let start = at(&state);
    let mut way = None;
    for k in [Key::Right, Key::Left, Key::Up, Key::Down] {
        seen.borrow_mut().clear();
        turn(&mut e, &[InputEvent::Key(k)], &mut feed);
        if at(&state) != start {
            way = Some(k);
            break;
        }
    }
    let way = way.quiet();
    assert!(
        seen.borrow().iter().any(|&(early, _, slid)| early && slid),
        "the step's early picture slid (the presenter slides here)"
    );
    let held = held_before(&p.borrow());
    assert!(held > 0, "the presenter holds a scene from before the load");
    loaded.set(true);
    crate::saveload::restore(&mut state.borrow_mut(), e.io()).quiet();
    {
        // As the town's setup, combat broken off in the state (0x121d78): no input before the step.
        let mut s = state.borrow_mut();
        pass_assault(&mut s);
        if s.actors.combat_mode() {
            crate::combat::mode::break_off(&mut s);
        }
    }
    assert_eq!(
        at(&state),
        start,
        "the restore put the party back a cell away"
    );
    assert!(!state.borrow().actors.combat_mode(), "out of combat");
    seen.borrow_mut().clear();
    let across = match way {
        Key::Right | Key::Left => [Key::Up, Key::Down],
        _ => [Key::Right, Key::Left],
    };
    let mut walked = false;
    for k in across {
        let from = at(&state);
        turn(&mut e, &[InputEvent::Key(k)], &mut feed);
        if at(&state) != from {
            walked = true;
            break;
        }
    }
    assert!(walked, "a step across the first one's way walked");
    let seen = seen.borrow();
    let first = *seen.first().quiet();
    assert!(
        first.0,
        "the first publication after the load is the step's early picture"
    );
    assert!(first.1 && !first.2, "it snaps, no slide into it: {first:?}");
    println!(
        "a step, a load a cell back, a step across as the first input: {} scenes published before \
         the load, {held} of them held at it; {} publications after the load, the first (the early \
         picture) snapped, no slide into it, and after each of them ({}) none of the scenes from \
         before held",
        before.borrow().len(),
        seen.len(),
        checked.get()
    );
}

/// Remembers every cell within 12 of the leader's on its level as last seen at `seen` (game
/// minutes, [`super::cache::minutes`]), its look the leader's cell's ground.
fn remember_round_at(state: &mut State, seen: u64) {
    let c = state.map.centre;
    let i = crate::geom::HALF_H * crate::geom::VIEW_W + crate::geom::HALF_W;
    let t = usize::from(state.objects.draw.base[i] & 0x7ff);
    let gfx = &mut state.gfx;
    let layer = super::cache::Layer {
        record: gfx.record_of[t],
        masktype: gfx.masktype[t],
        frame: 0,
        frozen: None,
    };
    for dy in -12i32..=12 {
        for dx in -12i32..=12 {
            let (x, y) = ((i32::from(c.x) + dx) as u16, (i32::from(c.y) + dy) as u16);
            gfx.cache.memory.remember((c.z, x, y), vec![layer], seen);
        }
    }
}

/// The pixels of `drawn` (a frame `s` device pixels a frame pixel) on the remembered cells of
/// `scene` (out of sight in it) drawn at `camera`, against `want`, the frame of the render of
/// `scene` at rest: each such pixel shows the one of its cell at rest, `camera` pixels on
/// ([`remembered_pixels`]). Returns (compared, differing).
fn remembered_against(
    drawn: &[u32],
    want: &[u32],
    s: usize,
    scene: &u6_render::motion::Scene,
    camera: (i32, i32),
    skip: &dyn Fn(i32, i32) -> bool,
) -> (usize, usize) {
    let pixels = remembered_pixels(s, scene, camera, skip);
    let differ = (pixels.iter())
        .filter(|&&(at, rest)| drawn[at] != want[rest])
        .count();
    (pixels.len(), differ)
}

/// The pixels of a frame `s` device pixels a frame pixel on the remembered cells of `scene` (out
/// of sight in it) drawn at `camera`, each as (its index on the screen, its index at rest). Only
/// pixels inside the shown map at rest and on the screen, and no frame pixel `skip` names (what is
/// drawn over the map there: the movers, the cursor and the border).
fn remembered_pixels(
    s: usize,
    scene: &u6_render::motion::Scene,
    camera: (i32, i32),
    skip: &dyn Fn(i32, i32) -> bool,
) -> Vec<(usize, usize)> {
    let (x0, y0, x1, y1) = SHOWN;
    let shown = |x: i32, y: i32| {
        (i32::from(x0)..=i32::from(x1)).contains(&x) && (i32::from(y0)..=i32::from(y1)).contains(&y)
    };
    let w = u6_render::WIDTH * s;
    let mut pixels = Vec::new();
    for (i, cell) in scene.cells.iter().enumerate() {
        let unseen =
            cell.base == crate::map::view::NOT_VISIBLE || cell.base == crate::map::view::DARK;
        let at = (
            scene.corner.0 + 16 * (i % scene.cols) as i32,
            scene.corner.1 + 16 * (i / scene.cols) as i32,
        );
        let screen = (at.0 - camera.0, at.1 - camera.1);
        if !unseen {
            continue;
        }
        for y in 0..16 * s {
            for x in 0..16 * s {
                let (fx, fy) = (screen.0 + (x / s) as i32, screen.1 + (y / s) as i32);
                let rest = (at.0 + (x / s) as i32, at.1 + (y / s) as i32);
                if !shown(fx, fy) || !shown(rest.0, rest.1) || skip(fx, fy) || skip(rest.0, rest.1)
                {
                    continue;
                }
                let index =
                    |(px, py): (i32, i32)| (py as usize * s + y % s) * w + px as usize * s + x % s;
                pixels.push((index((fx, fy)), index(rest)));
            }
        }
    }
    pixels
}

/// The calendar `minutes` game minutes on (the original's 60 minutes, 24 hours, 28 days, 12
/// months).
fn later(cal: &mut crate::actors::Calendar, minutes: u64) {
    for _ in 0..minutes {
        cal.minute += 1;
        if cal.minute < 60 {
            continue;
        }
        cal.minute = 0;
        cal.hour += 1;
        if cal.hour < 24 {
            continue;
        }
        cal.hour = 0;
        cal.day += 1;
        if cal.day <= 28 {
            continue;
        }
        cal.day = 1;
        cal.month += 1;
        if cal.month > 12 {
            cal.month = 1;
            cal.year += 1;
        }
    }
}

/// An engine picture: its shot, its classic pixels and its HD words at 2×.
type Picture = (Rc<Shot>, Vec<u32>, Vec<u32>);

/// The pictures `e` publishes with a scene from now on (`each`'s calls), drained: each with
/// [`Picture`]'s frames as published.
fn pictures(e: &mut Engine, hd: &mut HdView, out: &mut Vec<Picture>) {
    for publ in e.take_motion() {
        if let Some(shot) = publ.shot.filter(|_| !publ.preview) {
            hd.present(Some(e), &Framebuffer::new());
            let classic = e
                .published_frame()
                .pixels()
                .iter()
                .map(|&c| u32::from(c))
                .collect();
            out.push((shot, classic, hd.rgba().to_vec()));
        }
    }
}

/// Spec 18 and 20, on the user's zip: a fade step during a slide. In the town the remembered
/// cells round the party are seen d whole game days ago (d from 0 to 4: day 4 to 5 is the step to
/// black); the party steps along the town's walk until a step's picture (B) has cells out of
/// sight in the shown map (faded d); the calendar moved on to d + 1 days since the sight,
/// the prompt's refresh draws the cells faded d + 1, the same places (picture B'). A presenter
/// slides from the picture before the step into B, and B' comes half way. At the slide's start,
/// its middle (B' just in), near its end and at its end, the remembered cells drawn are the
/// engine's own pictures shifted by the camera. Classic: the plan, of the newest scene (B, then
/// B': an in-place change shows at the next present), at the end B' at rest. HD: the layers
/// made from B over the HD frame then, of B while the slide runs; at its end (the presenter no
/// longer active) they still show B and differ from B', so the page must show its HD frame then
/// (u6-web's page test of that switch). From the midpoint B and B' differ on the pixels compared,
/// classic and HD (but at the midpoint at 4 days in Original: why at the check), so the samples
/// tell them apart.
#[test]
fn a_fade_step_during_a_slide_shows_in_classic_at_once_and_in_hd_at_its_end() {
    let Some(files) = files() else { return };
    let s = 2;
    let mut totals = [0usize; 7];
    for days in 0..=4u64 {
        let mut hd = HdView::new();
        hd.set(true, Scale::X2);
        let mut game = boot(&files, 0, true);
        let mut pics = Vec::new();
        play(&mut game, true, |e| pictures(e, &mut hd, &mut pics));
        let a = Rc::clone(&pics.last().quiet().0);
        let now = super::cache::minutes(&game.1.borrow().actors.calendar);
        let seen = now - days * 1440;
        remember_round_at(&mut game.1.borrow_mut(), seen);
        let (mut e, state) = game;
        let centre = |s: &RefCell<State>| (s.borrow().map.centre.x, s.borrow().map.centre.y);
        // A step of the town's walk whose picture has cells out of sight whole in the shown map.
        let (mut b, mut a) = (None, a);
        for k in STEPS {
            let from = centre(&state);
            let mut stepped = Vec::new();
            turn(&mut e, &[InputEvent::Key(k)], &mut |e| {
                pictures(e, &mut hd, &mut stepped)
            });
            let Some(last) = stepped.pop() else { continue };
            let (cells, _) =
                remembered_against(&last.1, &last.1, 1, &last.0.scene, (16, 16), &|_, _| false);
            if centre(&state) != from && cells > 0 {
                b = Some(last);
                break;
            }
            a = Rc::clone(&last.0);
        }
        let b = b.quiet();
        // On to the fade step (d + 1 whole days since the sight; the steps took less than a day):
        // the prompt's refresh draws the faded cells again, in place.
        let at_b = super::cache::minutes(&state.borrow().actors.calendar);
        assert!(
            at_b < seen + (days + 1) * 1440,
            "{days} days: B before the fade step"
        );
        later(
            &mut state.borrow_mut().actors.calendar,
            seen + (days + 1) * 1440 - at_b,
        );
        let mut again = Vec::new();
        for _ in 0..200 {
            e.tick(&[], STEP_MS);
            pictures(&mut e, &mut hd, &mut again);
            if !again.is_empty() {
                break;
            }
        }
        let b2 = again.into_iter().next().quiet();
        assert!(
            b2.0.scene.placed == b.0.scene.placed && b2.0.scene.centre == b.0.scene.centre,
            "{days} days: the same places"
        );
        assert!(
            b2.1 != b.1,
            "{days} days: the fade step changed the picture"
        );
        // The slide into B, B' half way.
        let mut p: Presenter<Shot> = Presenter::new();
        p.publish(Rc::clone(&a), 0.0, true);
        let t0 = 1000.0;
        p.publish(Rc::clone(&b.0), t0, true);
        assert!(slides_at(&p, t0), "{days} days: the step slides");
        let g = p.generation();
        let slide = slide_layers(&e.state(), &p).quiet();
        let words: Vec<Vec<u32>> = (slide.parts.iter())
            .map(|part| {
                let mut w = Vec::new();
                hd.compose_layer(&e, &part.pixels, &mut w);
                w
            })
            .collect();
        let mut bad = Vec::new();
        let end = t0 + SLIDE_MS;
        // B against B' at the midpoint (classic, HD), and what the held HD layers draw of B at the
        // slide's end against B'.
        let (mut midpoint, mut held_at_end) = ([0usize; 2], 0);
        // Where the sample of B against B' holds none: the changed pixels at rest, those off the
        // shown map at the camera and those under the frame layer.
        let mut unsampled = None;
        for (k, dt) in [0.0, 90.0, 180.0, 270.0, 350.0, SLIDE_MS]
            .into_iter()
            .enumerate()
        {
            let t = t0 + dt;
            if dt == 180.0 {
                p.publish(Rc::clone(&b2.0), t, true);
                assert_eq!(p.generation(), g, "{days} days: in place, the layers kept");
            }
            // The presenter is active until the slide's end; then the page shows the engine's
            // frames (u6-web's side).
            let sliding = p.active(t);
            assert_eq!(sliding, t < end, "{days} days: the slide runs to its end");
            let view = p.view(t).quiet();
            assert!(
                sliding || view.camera == (0, 0),
                "{days} days: at rest at the end"
            );
            let newest = if dt < 180.0 { &b } else { &b2 };
            // The movers and the words over them, where drawn now (not the remembered cells').
            let layers = p.layers();
            let movers: Vec<(i32, i32)> = (layers.placed(&view).into_iter())
                .flat_map(|(part, (dx, dy))| {
                    let ops = match part {
                        u6_render::motion::Part::Static => &[][..],
                        u6_render::motion::Part::Mover(i) => &layers.movers[i].ops[..],
                        u6_render::motion::Part::Over(i) => &layers.movers[i].over[..],
                    };
                    ops.iter().map(move |o| (o.x + dx, o.y + dy))
                })
                .collect();
            // And the cursor and the border, over the map where the frame layer draws.
            let frame = (slide.parts.iter())
                .find(|l| l.role == super::motion::Role::Frame)
                .map(|l| Rc::clone(&l.pixels));
            let skip = |x: i32, y: i32| {
                let mover = |&(mx, my): &(i32, i32)| {
                    (mx..mx + 16).contains(&x) && (my..my + 16).contains(&y)
                };
                movers.iter().any(mover)
                    || (frame.as_ref()).is_some_and(|f| f.index((x, y)).is_some_and(|i| f.drawn[i]))
            };
            // Classic: the plan, drawn from the newest scene (at the end, at rest: B', the engine's
            // picture the page then shows, drawn apart from it).
            let mut fb = poisoned(&e);
            paint(&e.state(), &p, &view, &mut fb);
            let drawn: Vec<u32> = fb.pixels().iter().map(|&c| u32::from(c)).collect();
            let sampled = remembered_pixels(1, &newest.0.scene, view.camera, &skip);
            let (n, d) =
                remembered_against(&drawn, &newest.1, 1, &newest.0.scene, view.camera, &skip);
            totals[0] += n;
            if n == 0 || d > 0 {
                bad.push(("classic", k, n, d));
            }
            // B against B' on these pixels: from the midpoint the sample tells the two apart, but
            // at the midpoint at 4 days in Original (Wide's sample there holds some). There the
            // few pixels B' changes (the last grey ones going black) lie off the shown map at the
            // camera's half cell, or under the frame layer's words (the cursor and the border),
            // which the sample leaves out: checked where the sample holds none.
            let faded = |pixels: &[(usize, usize)], (b, b2): (&[u32], &[u32])| {
                (pixels.iter())
                    .filter(|&&(_, rest)| b[rest] != b2[rest])
                    .count()
            };
            let tells = dt > 180.0 || dt == 180.0 && (days < 4 || cfg!(feature = "wide"));
            if dt >= 180.0 {
                let f = faded(&sampled, (&b.1, &b2.1));
                totals[4] += f;
                midpoint[0] += if dt == 180.0 { f } else { 0 };
                if tells && f == 0 {
                    bad.push(("classic, B against B'", k, sampled.len(), f));
                }
                if f == 0 {
                    // The changed pixels of the remembered cells at rest, those on the shown map
                    // at this camera, and of these those under no mover: none under a mover, so
                    // what is on the map is all under the frame layer.
                    let none = |_: i32, _: i32| false;
                    let mover = |x: i32, y: i32| {
                        (movers.iter()).any(|&(mx, my)| {
                            (mx..mx + 16).contains(&x) && (my..my + 16).contains(&y)
                        })
                    };
                    let scene = &b2.0.scene;
                    let at_rest = faded(&remembered_pixels(1, scene, (0, 0), &none), (&b.1, &b2.1));
                    let on = faded(
                        &remembered_pixels(1, scene, view.camera, &none),
                        (&b.1, &b2.1),
                    );
                    let free = faded(
                        &remembered_pixels(1, scene, view.camera, &mover),
                        (&b.1, &b2.1),
                    );
                    unsampled = Some((k, at_rest, at_rest - on, free));
                    if !(at_rest > on && on == free && free > 0) {
                        bad.push(("why B against B' is 0", k, at_rest - on, free));
                    }
                }
            }
            // HD: the layers made from B over the HD frame now (the newest picture's, as the page
            // draws them). While the slide runs they show B. At its end they still show B, which
            // differs from B' there: the page shows its HD frame (B') instead, the switch u6-web's
            // `at_a_slides_end_the_page_shows_the_engines_hd_frame` exercises.
            let ow = u6_render::WIDTH * s;
            let mut out = newest.2.clone();
            for (i, x, y) in slide_placements(&slide, &view) {
                let (px, w) = (&slide.parts[i].pixels, &words[i]);
                let lw = px.width * s;
                for (j, &word) in w.iter().enumerate() {
                    let (dx, dy) = (
                        x * s as i32 + (j % lw) as i32,
                        y * s as i32 + (j / lw) as i32,
                    );
                    if word >> 24 != 0
                        && dx >= 0
                        && dy >= 0
                        && in_shown(dx as usize / s, dy as usize / s)
                    {
                        out[dy as usize * ow + dx as usize] = word;
                    }
                }
            }
            let sampled = remembered_pixels(s, &b.0.scene, view.camera, &skip);
            if sliding {
                let (n, d) = remembered_against(&out, &b.2, s, &b.0.scene, view.camera, &skip);
                totals[1] += n;
                if n == 0 || d > 0 {
                    bad.push(("HD", k, n, d));
                }
            } else {
                let (n, d) = remembered_against(&out, &b2.2, s, &b2.0.scene, view.camera, &skip);
                held_at_end += d;
                if n == 0 || d == 0 {
                    bad.push(("HD layers at the end, against B'", k, n, d));
                }
            }
            if dt >= 180.0 {
                let f = faded(&sampled, (&b.2, &b2.2));
                totals[5] += f;
                midpoint[1] += if dt == 180.0 { f } else { 0 };
                if tells && f == 0 {
                    bad.push(("HD, B against B'", k, sampled.len(), f));
                }
            }
        }
        assert!(!p.active(end), "{days} days: the slide over");
        let (x0, y0, x1, y1) = SHOWN;
        let grey = |f: &[u32]| {
            (usize::from(y0)..=usize::from(y1))
                .flat_map(|y| (usize::from(x0)..=usize::from(x1)).map(move |x| (x, y)))
                .filter(|&(x, y)| f[y * u6_render::WIDTH + x] >= u32::from(u6_render::GREY))
                .count()
        };
        assert!(
            grey(&b2.1) < grey(&b.1),
            "{days} days: the fade step took grey pixels away ({} then {})",
            grey(&b.1),
            grey(&b2.1)
        );
        assert!(
            bad.is_empty(),
            "{days} days: (renderer, sample, remembered pixels compared, differing): {bad:?}"
        );
        totals[2] += grey(&b.1);
        totals[3] += grey(&b2.1);
        totals[6] += held_at_end;
        // The case the midpoint's sample is left for is this run's: Original at 4 days, at the
        // midpoint (sample 2), and nowhere else (a zero elsewhere is a failure above).
        let midpoint_unsampled = days == 4 && !cfg!(feature = "wide");
        assert_eq!(
            unsampled.map(|u| u.0),
            midpoint_unsampled.then_some(2),
            "{days} days: the sample at the midpoint holds none only at 4 days in Original"
        );
        if let Some((_, at_rest, off, under)) = unsampled {
            println!(
                "{days} days: none of the {at_rest} pixels B' changes sampled at the midpoint: \
                 {off} off the shown map at the camera, {under} under the frame layer"
            );
        }
        println!(
            "{days} to {} days: grey pixels of the shown map {} then {}; at the midpoint {} \
             classic and {} HD pixels sampled differ between B and B'; at the end the held HD \
             layers differ from B' on {held_at_end}",
            days + 1,
            grey(&b.1),
            grey(&b2.1),
            midpoint[0],
            midpoint[1]
        );
    }
    println!(
        "a fade step during a slide, 0 to 4 days: {} classic pixels of the plan (the newest scene, \
         to the end) and {} HD pixels of the held layers (B, during the slide) drawn as the \
         engine's pictures; from the midpoint {} classic and {} HD of the pixels sampled differ \
         between B and B'; at the end the held HD layers differ from B' on {}",
        totals[0], totals[1], totals[4], totals[5], totals[6]
    );
}

/// Spec 20 Bounds, on the user's zip, in this build's layout: the heap bytes of a shot
/// ([`Shot::heap_bytes`], its accounting checked by hand in `gfx::motion`'s tests) of each scene
/// published in the start fight, on the town's walk, and on the town's walk with every cell within
/// 12 of the party remembered a day ago (each cell out of sight then draws remembered layers). The
/// most of them is a measured figure of these runs, not a bound: a scene with more nodes or more
/// remembered layers holds more. The totals are estimates from it: the most shots motion holds at
/// once (the engine's [`crate::engine::SHOTS`] kept, the one kept last and the early picture made
/// in the poll, [`crate::engine::PUBLISHED`] queued for the platform, or two a tick for
/// [`crate::pacer::MAX_STEPS`] ticks with a page's presents, and the presenter's
/// [`u6_render::motion::HISTORY`], an early picture held one of them), each that size.
#[test]
fn the_heap_bytes_of_a_shot() {
    let Some(files) = files() else { return };
    let mut most = 0;
    for (case, town, remembered) in [
        ("the start fight", false, false),
        ("the town's walk", true, false),
        ("the town's walk, remembered round", true, true),
    ] {
        let mut game = boot(&files, 0, true);
        if remembered {
            remember_round(&mut game.1.borrow_mut(), 1);
        }
        let mut sizes = Vec::new();
        let mut layers = 0;
        play(&mut game, town, |e| {
            for publ in e.take_motion() {
                if let Some(shot) = publ.shot {
                    sizes.push(shot.heap_bytes());
                    layers += (shot.memory.iter()).map(|m| m.layers.len()).sum::<usize>();
                }
            }
        });
        assert!(!sizes.is_empty(), "{case}: scenes published");
        assert!(
            !remembered || layers > 0,
            "{case}: remembered layers in the scenes"
        );
        sizes.sort_unstable();
        let max = *sizes.last().quiet();
        most = most.max(max);
        println!(
            "{case}: {} scenes, heap bytes a shot median {} max {}, remembered layers {layers}",
            sizes.len(),
            sizes[sizes.len() / 2],
            max
        );
    }
    let (engine, queued) = (crate::engine::SHOTS + 2, crate::engine::PUBLISHED);
    let paced = 2 * crate::pacer::MAX_STEPS as usize;
    let history = u6_render::motion::HISTORY;
    println!(
        "the most a shot held in these runs (measured): {most} bytes; at most {} shots at once \
         ({engine} in the engine, {queued} queued, {history} in the presenter): about {} bytes \
         (an estimate, each at that size); with a page's presents ({paced} queued at most): {} \
         shots, about {} bytes",
        engine + queued + history,
        (engine + queued + history) * most,
        engine + paced + history,
        (engine + paced + history) * most
    );
}

/// Spec 20 Bounds (C2's merge note 6: spec 18's memory figure is the remembered sight's alone,
/// with motion off), on the user's zip, in this build's layout: the most heap bytes of the shots
/// motion holds at once over a run with it on. The runs: the start fight, and the town's walk
/// with every cell within 12 of the party remembered a day ago; a presenter fed as a page at 8×
/// feeds it (the publications taken every 8 ticks). After each tick, and on a tick that feeds it
/// also before the feed (the presenter's shots beside the queued ones it has not taken yet): the
/// shots the engine holds ([`Engine::motion_shots`]) and those the presenter holds, each once
/// ([`Shot::heap_bytes`]).
#[test]
fn the_shots_motion_holds_at_its_peak() {
    let Some(files) = files() else { return };
    // The most bytes held, the shots then, and the most shots held.
    let mut peak = (0usize, 0usize, 0usize);
    for (case, town) in [
        ("the start fight", false),
        ("the town's walk, remembered", true),
    ] {
        let mut game = boot(&files, 0, true);
        if town {
            remember_round(&mut game.1.borrow_mut(), 1);
        }
        let mut p: Presenter<Shot> = Presenter::new();
        let (mut ticks, mut most) = (0usize, (0usize, 0usize, 0usize));
        // Samples taken before a feed, and those of them that were the most so far.
        let (mut before, mut peaks_before) = (0usize, 0usize);
        let mut sample = |e: &Engine, p: &Presenter<Shot>, fed: bool| {
            let mut held = e.motion_shots();
            held.extend(p.history().iter().cloned());
            let mut once: Vec<Rc<Shot>> = Vec::new();
            for s in held {
                if !once.iter().any(|o| Rc::ptr_eq(o, &s)) {
                    once.push(s);
                }
            }
            let bytes = once.iter().map(|s| s.heap_bytes()).sum::<usize>();
            if bytes > most.0 {
                most = (bytes, once.len(), most.2);
                peaks_before += usize::from(fed);
            }
            most.2 = most.2.max(once.len());
        };
        play(&mut game, town, |e| {
            ticks += 1;
            if ticks % 8 == 0 {
                sample(e, &p, true);
                before += 1;
                for publ in e.take_motion() {
                    let time = publ.time_ms as f64;
                    match publ.shot {
                        None => p.foreign(),
                        Some(shot) if publ.preview => p.preview(shot, time, true),
                        Some(shot) => p.publish(shot, time, true),
                    }
                }
            }
            sample(e, &p, false);
        });
        assert!(
            most.0 > 0 && before > 0,
            "{case}: shots held, sampled before a feed"
        );
        println!(
            "{case}: {ticks} ticks, {before} sampled before a feed too; the most held at once {} \
             bytes in {} shots; the most shots {}; {peaks_before} new peaks before a feed",
            most.0, most.1, most.2
        );
        if most.0 > peak.0 {
            peak = (most.0, most.1, peak.2);
        }
        peak.2 = peak.2.max(most.2);
    }
    println!(
        "the shots motion holds with it on, at its peak over the runs: {} bytes in {} shots; the \
         most shots at once {}",
        peak.0, peak.1, peak.2
    );
}

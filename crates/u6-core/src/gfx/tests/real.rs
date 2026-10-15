//! Real data: the user's zip (skipped when absent; nothing is copied, only numbers asserted).
#![allow(clippy::disallowed_methods)] // tests may read files

use std::path::Path;

use u6_formats::gfx::TileKind;

use crate::engine::Engine;
use crate::files::Files;
use crate::gfx::GfxState;
use crate::gfx::anim::shore_mask;
use crate::gfx::seams::view_window;
use crate::gfx::tables::GameTables;
use crate::state::State;
use u6_testkit::Quiet;
use u6_testkit::assert_same;

/// The window render with `g` as the state's graphics for the call.
fn render_window(s: &mut State, g: &mut GfxState, io: &crate::engine::Io, framed: bool) {
    std::mem::swap(&mut s.gfx, g);
    crate::gfx::render_window(s, io, framed);
    std::mem::swap(&mut s.gfx, g);
}

/// The refresh with `g` as the state's graphics for the call.
#[allow(dead_code)]
fn refresh_view(s: &mut State, g: &mut GfxState, io: &crate::engine::Io) {
    std::mem::swap(&mut s.gfx, g);
    crate::gfx::refresh_view(s, io);
    std::mem::swap(&mut s.gfx, g);
}

/// The animation tick with `g` as the state's graphics for the call.
fn anim_tick(s: &mut State, g: &mut GfxState) {
    std::mem::swap(&mut s.gfx, g);
    crate::gfx::anim::anim_tick(s);
    std::mem::swap(&mut s.gfx, g);
}

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
/// The flattened image REA reads (local, derived; only compared against, never copied).
const IMAGE: &str = "/Users/zhugehyuk/Games/ultima6/re/game.bin";

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

#[test]
fn real_tables_match_the_image() {
    let Some(files) = files() else { return };
    let t = GameTables::from_game(files.get("game").quiet()).quiet();
    if let Ok(img) = std::fs::read(IMAGE) {
        let at = |a: usize, n: usize| img[a - 0x0010_0000..a - 0x0010_0000 + n].to_vec();
        let be = |v: &[u16]| v.iter().flat_map(|w| w.to_be_bytes()).collect::<Vec<_>>();
        assert_same!(
            be(&t.shore_src),
            at(0x0015_2C52, 64),
            "the shore sources: no relocation in the table"
        );
        assert_same!(be(&t.shore_dst), at(0x0015_2C92, 64), "the shore targets");
        assert_same!(
            be(&t.dissolve_taps),
            at(0x0015_1F3C, 30),
            "the dissolve taps"
        );
        let mut words = at(0x0015_5180, 2);
        words.extend(at(0x0015_3BF8, 2));
        words.extend(at(0x0015_5178, 2));
        assert_same!(
            be(&[t.window_colour, t.window_shown, t.dissolve]),
            words,
            "the window colour, shown flag and dissolve words"
        );
    } else {
        eprintln!("image comparison skipped: {IMAGE} not present");
    }
    // Shore merges: the source tiles, the 32 shore targets (graphics.md: tiles 16..48).
    let mut src: Vec<u16> = t.shore_src.iter().map(|w| w / 4).collect();
    src.sort_unstable();
    src.dedup();
    assert!(!src.is_empty(), "shore sources");
    assert!(t.shore_dst.iter().chain(&t.shore_src).all(|w| w % 4 == 0));
    // A flag word (its image value is compared above when the flattened image is present).
    assert!(t.window_shown <= 1, "the window-shown flag");
}

#[test]
fn real_shore_masks_fit_and_the_window_draws_the_party() {
    let Some(files) = files() else { return };
    let mut state = State::new_game(&files).quiet();
    crate::main_loop::load_state_tail(&mut state);
    let mut gfx = GfxState::load(&files).quiet();
    let maps = gfx
        .records
        .iter()
        .filter(|r| r.kind == TileKind::Map)
        .count();
    // The map tiles are those whose masktype byte has `& 3 == 0` (graphics.md §4), counted in
    // the user's masktype file.
    let masktype = u6_formats::lzw::decode(files.get("masktype.ega").quiet()).quiet();
    assert!(maps == masktype.iter().filter(|&&m| m & 3 == 0).count());
    // Two ticks: the animdata frames reach the water tiles, the second (even) one merges.
    anim_tick(&mut state, &mut gfx);
    anim_tick(&mut state, &mut gfx);
    for i in 0..32 {
        let (skip, masks) = shore_mask(&gfx.animmask.block2, i).expect("mask record");
        assert!(skip % 2 == 0);
        let rows = usize::from(skip / 2) + masks.len();
        for w in [gfx.tables.shore_src[i], gfx.tables.shore_dst[i]] {
            let (rec, _, mt) = gfx.record(w / 4);
            assert!((rec.kind, mt & 3) == (TileKind::Map, 0), "merge {i}");
            assert!(
                rows <= usize::from(rec.rows),
                "merge {i}: the mask fits the tile"
            );
        }
    }
    let window = view_window(&state);
    let centre = &window.objects[5 * 11 + 5];
    let leader = crate::objects::ObjectId(1);
    let hero = state.tables.basetile.base[usize::from(state.objects.obj_type(leader))]
        + u16::from(state.objects.frame(leader));
    assert!(
        centre.last() == Some(&hero),
        "leader drawn last in cell (5, 5)"
    );
    let (rec, _, _) = gfx.record(hero);
    let expect: Vec<(usize, usize, u8)> = (0..256)
        .filter(|&i| !rec.transparent[i])
        .map(|i| (80 + i % 16, 80 + i / 16, rec.pixels[i]))
        .collect();
    let e = {
        let mut e = Engine::with_game(State::empty(), files.clone(), move |_s, io| async move {
            render_window(&mut state, &mut gfx, &io, true);
        });
        e.tick(&[], 0);
        e
    };
    let fb = e.frame();
    assert!(expect.len() > 50);
    for (x, y, p) in expect {
        assert!(fb.get(x, y) == Some(p), "hero pixel at ({x}, {y})");
    }
}

/// Spec 06 O2 on the user's game (`game` started at once, seed 0): 3000 steps with the render
/// cache and without it, idle at the prompt with four walks between; the shown frame and the back
/// buffer (the border's outer halves too) after every step and the whole `State` every 10 steps
/// are the same. The cache drew less than the whole
/// window, and no shore merge is crossed; only counts are printed.
#[test]
fn real_idle_with_the_cache_shows_what_it_shows_without() {
    let Some(files) = files() else { return };
    let mut on = Engine::game_first(files.clone(), 0).quiet();
    let mut off = Engine::game_first(files, 0).quiet();
    off.io().set_window_cache(false);
    let walks = [
        (500, crate::Key::Right),
        (900, crate::Key::Down),
        (1300, crate::Key::Left),
        (1700, crate::Key::Up),
    ];
    let mut crossed = false;
    for step in 0..3000u32 {
        let input: Vec<crate::InputEvent> = walks
            .iter()
            .filter(|&&(at, _)| at == step)
            .map(|&(_, k)| crate::InputEvent::Key(k))
            .collect();
        on.tick(&input, 20);
        off.tick(&input, 20);
        assert_same!(
            on.frame().pixels(),
            off.frame().pixels(),
            "step {step}: frame"
        );
        assert!(
            on.frame().palette == off.frame().palette,
            "step {step}: palette"
        );
        let back = |e: &Engine| e.io().peek_back(|b| b.pixels().to_vec());
        assert_same!(back(&on), back(&off), "step {step}: the back buffer");
        if step % 10 == 9 {
            assert!(*on.state() == *off.state(), "step {step}: the whole State");
        }
        crossed |= crate::gfx::cache::shores_crossed(&on.state().gfx);
    }
    let (c, o) = (&on.state().gfx.cache, &off.state().gfx.cache);
    println!(
        "3000 steps: renders drawing nothing {}, drawing {} boxes in {}; shore merges left \
         out {}; without the cache {} {} {}",
        c.window_skips,
        c.boxes_redrawn,
        c.window_partial,
        c.shore_skips,
        o.window_skips,
        o.window_partial,
        o.shore_skips
    );
    assert!(
        c.window_partial > 1000,
        "most refreshes draw some boxes only"
    );
    assert!(!crossed, "no shore merge writes a record another reads");
    assert!((o.window_skips, o.window_partial, o.shore_skips) == (0, 0, 0));
}

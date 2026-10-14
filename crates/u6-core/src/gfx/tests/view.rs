//! Window render, refresh, animation tick and the view-window fallback on a synthetic world.

use u6_formats::animdata::{AnimData, AnimEntry};
use u6_formats::basetile::BaseTiles;
use u6_formats::chunks::Chunks;
use u6_formats::gfx::Span;
use u6_formats::map::{SURFACE_CHUNKS, WorldMap};
use u6_formats::position::Position;
use u6_formats::tileflag::TileFlags;

use super::{install, map_tile, object_tile};
use crate::Files;
use crate::engine::Engine;
use crate::gfx::GfxState;
use crate::state::State;

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

const AVATAR_TYPE: usize = 0x19a;
const AVATAR_BASE: u16 = 1000;

/// Surface of chunk 1 everywhere, chunk 1 tile (col, row) = row·8 + col; map tile n has colour
/// n % 32 + 1 in every pixel; the leader (actor 1) stands at (19, 10).
fn world() -> (State, GfxState) {
    let mut s = State::empty();
    s.map.world = WorldMap {
        surface: vec![1; SURFACE_CHUNKS * SURFACE_CHUNKS],
        dungeons: Vec::new(),
    };
    let mut chunks = vec![[[0u8; 8]; 8]; 2];
    for (r, row) in chunks[1].iter_mut().enumerate() {
        for (c, t) in row.iter_mut().enumerate() {
            *t = (r * 8 + c) as u8;
        }
    }
    s.map.chunks = Chunks { chunks };
    s.tables.tileflags = TileFlags {
        flags1: vec![0; 2048],
        flags2: vec![0; 2048],
        weight: vec![0; 1024],
        flags3: vec![0; 2048],
    };
    let mut base = vec![0u16; 1024];
    base[AVATAR_TYPE] = AVATAR_BASE;
    s.tables.basetile = BaseTiles { base };
    s.map.centre = Position { x: 19, y: 10, z: 0 };
    s.objects.location[1] = s.map.centre;
    s.objects.type_frame[1] = AVATAR_TYPE as u16;
    let mut g = GfxState::default();
    for n in 0..64 {
        install(&mut g, n, map_tile(0, 16, |_, _| (n % 32 + 1) as u8));
    }
    install(
        &mut g,
        usize::from(AVATAR_BASE),
        object_tile(Span::Full, |r, c| (r == 8 && c == 8).then_some(30)),
    );
    g.window_shown = 1;
    prepare(&mut s);
    (s, g)
}

/// Load state's tail for the synthetic world: the object list over the actors, the window cache
/// around the centre (0x108d10) and the view with its draw lists (0x109e74 → 0x10e81e), which
/// the window render reads. Run again after moving the centre or an actor.
fn prepare(s: &mut State) {
    crate::objects::init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    s.map.window_key = [-1; 3];
    crate::map::region::rebuild_window(s);
    crate::map::view::update_view(s);
}

/// Runs `f` inside a one-tick engine so `Io` exists; returns the published frame.
fn with_io(f: impl FnOnce(&crate::engine::Io) + 'static) -> Engine {
    let mut e = Engine::with_game(State::empty(), Files::default(), move |_s, io| async move {
        f(&io);
    });
    e.tick(&[], 0);
    e
}

#[test]
fn window_cells_follow_the_centre_and_the_leader_sits_at_80_96() {
    let (mut s, mut g) = world();
    let e = with_io(move |io| render_window(&mut s, &mut g, io, true));
    let fb = e.frame();
    // Cell (5, 5) = world (19, 10): chunk tile 2·8 + 3 = 19 → colour 20.
    assert_eq!(fb.get(80, 81), Some(20));
    assert_eq!(fb.get(95, 95), Some(20));
    // Cell (0, 0) = world (14, 5): tile 5·8 + 6 = 46 → colour 46 % 32 + 1 = 15; only pixels
    // 8..168 reach the screen (0x151f5a).
    assert_eq!(fb.get(8, 8), Some(15));
    assert_eq!(fb.get(0, 0), Some(0));
    // The avatar's one opaque pixel, over the map tile.
    assert_eq!(fb.get(88, 88), Some(30));
    // Nothing past the 11 cells.
    assert_eq!(fb.get(176, 100), Some(0));
}

#[test]
fn border_tiles_and_dissolve_flag() {
    let (mut s, mut g) = world();
    install(&mut g, 0x1b0, object_tile(Span::Right, |_, _| Some(25)));
    install(&mut g, 0x1b7, object_tile(Span::Left, |_, _| Some(26)));
    g.dissolve = 1;
    let mut e = with_io(move |io| {
        render_window(&mut s, &mut g, io, true);
        assert_eq!(g.dissolve, 0, "cleared after the dissolve");
    });
    let fb = e.frame();
    // The border's outer half (x or y below 8) stays in the back buffer: 0x1520e2 dissolves only
    // (8, 8)..(0xa7, 0xa7) to the screen.
    assert_eq!((fb.get(8, 8), fb.get(7, 8)), (Some(25), Some(0)));
    // 0x1b7 at (0xa0, 16 i), left half: x 160..168 for y 16..160.
    assert_eq!((fb.get(160, 16), fb.get(167, 159)), (Some(26), Some(26)));
    assert_ne!(fb.get(160, 15), Some(26));
    let hits = e.stub_hits();
    assert!(
        !hits
            .keys()
            .any(|k| k.starts_with("0x1520e2") || k.starts_with("0x106932"))
    );
    assert!(!e.take_log().iter().any(|l| l.contains("0x106932")));
}

#[test]
fn sparkle_draws_two_randoms_per_pixel() {
    let (mut s, mut g) = world();
    g.sparkle = 1;
    let mut rng = s.rng;
    for _ in 0..200 {
        rng.range(8, 0xa7);
    }
    let e = with_io(move |io| {
        render_window(&mut s, &mut g, io, false);
        assert_eq!(s.rng, rng);
        assert_eq!((g.plot_colour, g.plot_xor, g.plot_back), (8, false, false));
    });
    // Unframed: drawn into the back buffer, never presented.
    assert_eq!(e.frame().get(80, 80), Some(0));
}

#[test]
fn refresh_runs_the_tick_and_the_window_only_when_shown() {
    let (mut s, mut g) = world();
    g.window_shown = 0;
    s.map.view_dirty = 1;
    g.panel_redraw = 3;
    let e = with_io(move |io| {
        refresh_view(&mut s, &mut g, io);
        assert_eq!(
            (s.map.view_dirty, g.panel_redraw, g.anim_counter),
            (0, 0, 0)
        );
        g.window_shown = 1;
        refresh_view(&mut s, &mut g, io);
        assert_eq!(g.anim_counter, 1);
    });
    assert_eq!(e.frame().get(80, 81), Some(20));
    assert!(!e.stub_hits().keys().any(|k| k.starts_with("0x133a1a")));
}

#[test]
fn tick_substitutes_frames_by_mode_and_merges_on_even_counts() {
    let (mut s, mut g) = world();
    let slot = |tile| AnimEntry {
        tile_to_animate: tile,
        first_anim_frame: 40,
        and_mask: 0x0c,
        shift: 2,
    };
    s.tables.anim = AnimData {
        count: 3,
        slots: vec![slot(100), slot(101), slot(102), slot(103)],
    };
    s.map.anim_enabled = vec![1, 0, 1];
    g.anim_counter = 6;
    g.actor_anim_due = 1;
    // The leader (type 0x19a, frame 0) is the only actor: one roll(0, 0x1f) for its idle cycle.
    let mut rng = s.rng;
    let r = rng.range(0, 0x1f);
    anim_tick(&mut s, &mut g);
    assert_eq!(s.rng, rng);
    assert_eq!(
        s.objects.frame(crate::objects::ObjectId(1)),
        u8::from(r == 0)
    );
    // Counter 7: (7 & 0xc) >> 2 = 1 → frame 41; slot 1 is off → 40; slot 3 is past `count`.
    assert_eq!(g.anim_counter, 7);
    assert_eq!(g.record_of[100..104], [41, 40, 41, 103]);
    assert_eq!(g.actor_anim_due, 0);
    g.anim_paused = 1;
    anim_tick(&mut s, &mut g);
    assert_eq!(g.anim_counter, 7, "paused: no tick");
}

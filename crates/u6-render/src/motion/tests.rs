//! The presenter's receipts (spec 20): the camera's slide and snap, the movers matched by slot,
//! the occlusion order as frames, the restart and the snap of a move during a slide, the end of
//! every slide at a foreign publication, and the slide's length. Synthetic scenes only.

use super::*;
use crate::Framebuffer;
use std::collections::BTreeMap;

/// The view of the scenes here: 11 × 11 cells from frame pixel (0, 0), Original's.
const SIDE: usize = 11;
/// Base word of every cell (painted in [`BASE`]).
const GROUND: u16 = 1;
/// The actor word every object here is drawn with (unless a test says otherwise).
const ACTOR: u16 = 0x180;
/// The leader's slot.
const LEADER: u16 = 1;
/// The colour a base paints.
const BASE: u8 = 1;
/// The colour [`ACTOR`] paints, its whole cell.
const BODY: u8 = 2;

/// A scene round `centre` on level `z` with `objects` (slot, world cell, type), each drawn with
/// [`ACTOR`] in its own cell, the leader [`LEADER`].
fn scene(centre: (i32, i32), z: u8, objects: &[(u16, (i32, i32), u16)]) -> Scene {
    let half = (SIDE / 2) as i32;
    let mut s = Scene {
        leader: Some(LEADER),
        centre: (centre.0, centre.1, z),
        origin: (centre.0 - half, centre.1 - half),
        cols: SIDE,
        rows: SIDE,
        corner: (0, 0),
        cells: vec![
            Cell {
                base: GROUND,
                nodes: Vec::new(),
            };
            SIDE * SIDE
        ],
        placed: Vec::new(),
        snap: false,
    };
    for &(slot, cell, kind) in objects {
        add(&mut s, cell, ACTOR, Some(slot), Rank::Own);
        place(&mut s, slot, cell, kind);
    }
    s
}

/// Lists `slot` among the scene's objects, kept by slot.
fn place(s: &mut Scene, slot: u16, cell: (i32, i32), kind: u16) {
    s.placed.push(Placed { slot, cell, kind });
    s.placed.sort_by_key(|p| p.slot);
}

/// Appends a node to the list of world cell `at`.
fn add(s: &mut Scene, at: (i32, i32), word: u16, slot: Option<u16>, rank: Rank) {
    let i = s.index(at).expect("a cell of the view");
    s.cells[i].nodes.push(Node { word, slot, rank });
}

/// A presenter that showed `first` (published at 0) and then `second` at `at`.
fn two(first: Scene, second: Scene, at: f64) -> Presenter<Scene> {
    let mut p = Presenter::new();
    p.publish(Rc::new(first), 0.0, true);
    p.publish(Rc::new(second), at, true);
    p
}

fn offset_of(view: &View, slot: u16) -> Option<(i32, i32)> {
    view.movers
        .iter()
        .find(|m| m.slot == slot)
        .map(|m| m.offset)
}

#[test]
fn a_step_of_one_cell_slides_the_camera_and_anything_else_snaps() {
    let start = scene((100, 100), 0, &[(LEADER, (100, 100), 0x180)]);
    let east = scene((101, 100), 0, &[(LEADER, (101, 100), 0x180)]);
    let p = two(start.clone(), east.clone(), 1000.0);
    let at = |t: f64| p.view(t).map(|v| (v.camera, offset_of(&v, LEADER)));
    assert_eq!(at(1000.0), Some(((-16, 0), Some((-16, 0)))), "at its start");
    assert_eq!(at(1180.0), Some(((-8, 0), Some((-8, 0)))), "half way");
    assert!(p.active(1359.0) && !p.active(1360.0), "it lasts SLIDE_MS");
    assert_eq!(at(1360.0), Some(((0, 0), Some((0, 0)))), "at its end");
    // A diagonal step is one cell too.
    let ne = scene((101, 99), 0, &[(LEADER, (101, 99), 0x180)]);
    let p = two(start.clone(), ne, 0.0);
    assert_eq!(p.view(90.0).map(|v| v.camera), Some((-12, 12)));
    // Everything else snaps: nothing slides, the engine's frame is shown.
    let leader_at = |c: (i32, i32), z: u8, leader: u16| {
        let mut s = scene(c, z, &[(leader, c, 0x180)]);
        s.leader = Some(leader);
        s
    };
    let snaps = [
        ("a teleport", leader_at((102, 100), 0, LEADER)),
        ("a ladder", leader_at((100, 100), 1, LEADER)),
        ("a level change", leader_at((101, 100), 5, LEADER)),
        ("a switch of leader", leader_at((101, 100), 0, 2)),
        (
            "a dissolve",
            Scene {
                snap: true,
                ..east.clone()
            },
        ),
    ];
    for (what, to) in snaps {
        let p = two(start.clone(), to, 1000.0);
        assert!(
            p.view(1000.0).is_none() && !p.active(1000.0),
            "{what} snaps"
        );
    }
    let mut p = Presenter::new();
    p.publish(Rc::new(start), 0.0, true);
    p.publish(Rc::new(east), 1000.0, false);
    assert!(
        p.view(1000.0).is_none(),
        "no slide where slides are not shown (Max)"
    );
}

#[test]
fn movers_are_matched_by_slot_a_party_line_and_a_swap_slide_a_type_change_snaps() {
    let before = scene(
        (100, 100),
        0,
        &[
            (LEADER, (100, 100), 0x180),
            (2, (99, 100), 0x181),
            (3, (98, 100), 0x182),
            (4, (96, 103), 0x183),
            (5, (97, 103), 0x184),
            (6, (103, 97), 0x185),
            (7, (103, 98), 0x186),
        ],
    );
    let after = scene(
        (101, 100),
        0,
        &[
            (LEADER, (101, 100), 0x180),
            (2, (100, 100), 0x181),
            (3, (99, 100), 0x182),
            (4, (97, 103), 0x183),
            (5, (96, 103), 0x184),
            (6, (103, 98), 0x187),
            (7, (103, 100), 0x186),
            (8, (104, 100), 0x188),
        ],
    );
    let p = two(before, after, 0.0);
    let v = p.view(0.0).expect("a slide");
    for slot in [LEADER, 2, 3] {
        assert_eq!(
            offset_of(&v, slot),
            Some((-16, 0)),
            "the party line: slot {slot}"
        );
    }
    assert_eq!(offset_of(&v, 4), Some((-16, 0)), "a swap: one way");
    assert_eq!(offset_of(&v, 5), Some((16, 0)), "a swap: the other way");
    assert_eq!(offset_of(&v, 6), None, "a type change snaps");
    assert_eq!(offset_of(&v, 7), None, "a move of two cells snaps");
    assert_eq!(offset_of(&v, 8), None, "an object that appears snaps");
    assert_eq!(v.movers.len(), 5);
}

/// The colour and the rows (from the top) word `w` paints in its cell: the actor its whole cell,
/// [`ARCH`] (a doorway's top tile) its top 8 rows, [`WALL`] and [`WALL_PART`] (a corner's top
/// tile and a multi-tile part) their whole cell.
fn look(w: u16) -> (u8, usize) {
    match w {
        ACTOR => (BODY, 16),
        ARCH => (3, 8),
        WALL => (4, 16),
        WALL_PART => (5, 16),
        _ => (9, 16),
    }
}
const ARCH: u16 = 0x1a0;
const WALL: u16 = 0x1a1;
const WALL_PART: u16 = 0x1a2;

/// The plan at `t` painted over a cleared frame ([`look`]).
fn frame_at(p: &Presenter<Scene>, t: f64) -> Framebuffer {
    let view = p.view(t).expect("a slide");
    painted(&p.plan(&view))
}

/// `ops` painted over a cleared frame ([`look`]).
fn painted(ops: &[Op]) -> Framebuffer {
    let mut fb = Framebuffer::new();
    for &op in ops {
        let (colour, rows) = match op.draw {
            Draw::Base { .. } => (BASE, 16),
            Draw::Word(w) => look(w),
        };
        for r in 0..rows as i32 {
            for c in 0..CELL {
                let (x, y) = (op.x + c, op.y + r);
                if x >= 0 && y >= 0 {
                    fb.set(x as usize, y as usize, colour);
                }
            }
        }
    }
    fb
}

/// The colours of the 16 rows of column `col` of world cell `cell` in `fb` (the camera at rest).
fn column(fb: &Framebuffer, s: &Scene, cell: (i32, i32), col: i32) -> Vec<u8> {
    let (x, y) = s.pixel(cell);
    (0..CELL)
        .map(|r| fb.get((x + col) as usize, (y + r) as usize).unwrap_or(0))
        .collect()
}

fn rows_of(top: u8, n: usize, bottom: u8) -> Vec<u8> {
    let mut v = vec![top; n];
    v.resize(16, bottom);
    v
}

#[test]
fn a_mover_stays_under_a_doorway_top_at_its_start_half_way_and_its_end() {
    // The mover (slot 2) steps south into the doorway cell (100, 98), whose list has the
    // doorway's top tile behind it; the leader stands.
    let (above, door) = ((100, 97), (100, 98));
    let mut before = scene(
        (100, 100),
        0,
        &[(LEADER, (100, 100), 0x180), (2, above, 0x181)],
    );
    add(&mut before, door, ARCH, Some(9), Rank::Top);
    place(&mut before, 9, door, 0x10);
    let mut after = scene(
        (100, 100),
        0,
        &[(LEADER, (100, 100), 0x180), (2, door, 0x181)],
    );
    add(&mut after, door, ARCH, Some(9), Rank::Top);
    place(&mut after, 9, door, 0x10);
    let s = after.clone();
    let p = two(before, after, 0.0);
    let (t0, mid, end) = (
        frame_at(&p, 0.0),
        frame_at(&p, 180.0),
        frame_at(&p, SLIDE_MS),
    );
    // t0: the mover whole in the cell above, the arch over the ground below.
    assert_eq!(column(&t0, &s, above, 4), vec![BODY; 16]);
    assert_eq!(column(&t0, &s, door, 4), rows_of(3, 8, BASE));
    // Half way: its lower half in the doorway's top rows, under the arch.
    assert_eq!(column(&mid, &s, above, 4), rows_of(BASE, 8, BODY));
    assert_eq!(
        column(&mid, &s, door, 4),
        rows_of(3, 8, BASE),
        "under the arch"
    );
    // The end: in the doorway, its top under the arch, as the engine draws it.
    assert_eq!(column(&end, &s, above, 4), vec![BASE; 16]);
    assert_eq!(column(&end, &s, door, 4), rows_of(3, 8, BODY));
}

#[test]
fn a_diagonal_step_past_a_wall_corner_stays_under_the_corner() {
    // The mover (slot 2) steps north-east from (102, 103) to (103, 102); the side cells hold a
    // corner's top tile (north of its old cell) and a wall's multi-tile part (east of it).
    let (from, to) = ((102, 103), (103, 102));
    let (north, east) = ((102, 102), (103, 103));
    let build = |at| {
        let mut s = scene(
            (100, 100),
            0,
            &[(LEADER, (100, 100), 0x180), (2, at, 0x181)],
        );
        add(&mut s, north, WALL, Some(10), Rank::Top);
        add(&mut s, east, WALL_PART, Some(11), Rank::Part);
        place(&mut s, 10, north, 0x20);
        place(&mut s, 11, (104, 103), 0x21);
        s
    };
    let s = build(to);
    let p = two(build(from), build(to), 0.0);
    let (t0, mid, end) = (
        frame_at(&p, 0.0),
        frame_at(&p, 180.0),
        frame_at(&p, SLIDE_MS),
    );
    let at = |fb: &Framebuffer, cell, col: i32, row: i32| {
        let (x, y) = s.pixel(cell);
        fb.get((x + col) as usize, (y + row) as usize)
    };
    // t0: whole in its old cell.
    assert_eq!(at(&t0, from, 8, 8), Some(BODY));
    assert_eq!(at(&t0, to, 8, 8), Some(BASE));
    // Half way it overlaps four cells by a quarter each: seen in its old and new cells, under the
    // corner's top tile and the wall's part in the side cells.
    assert_eq!(
        at(&mid, from, 12, 4),
        Some(BODY),
        "the old cell's top right"
    );
    assert_eq!(
        at(&mid, to, 4, 12),
        Some(BODY),
        "the new cell's bottom left"
    );
    assert_eq!(at(&mid, north, 12, 12), Some(4), "under the corner");
    assert_eq!(at(&mid, east, 4, 4), Some(5), "under the wall's part");
    assert_eq!(
        at(&mid, from, 4, 12),
        Some(BASE),
        "the old cell's bottom left is clear"
    );
    // The end: whole in its new cell.
    assert_eq!(at(&end, to, 8, 8), Some(BODY));
    assert_eq!(at(&end, from, 8, 8), Some(BASE));
}

#[test]
fn a_move_during_a_slide_restarts_where_the_mover_is_drawn_even_past_a_cell_and_a_half() {
    let at = |x: i32| {
        scene(
            (100, 100),
            0,
            &[(LEADER, (100, 100), 0x180), (2, (x, 95), 0x181)],
        )
    };
    // From 96 to 97 at 0; half way (drawn at 96.5) the next step to 98: 1.5 cells, restarted.
    let mut p = two(at(96), at(97), 0.0);
    p.publish(Rc::new(at(98)), 180.0, true);
    let v = p.view(180.0).expect("restarted");
    assert_eq!(offset_of(&v, 2), Some((-24, 0)), "from where it is drawn");
    assert_eq!(p.view(360.0).and_then(|v| offset_of(&v, 2)), Some((-12, 0)));
    assert!(
        p.active(539.0) && !p.active(540.0),
        "a whole slide from the restart"
    );
    // A one-cell logical move remains continuous even almost two cells from its drawn place.
    let mut p = two(at(96), at(97), 0.0);
    p.publish(Rc::new(at(98)), 10.0, true);
    assert_eq!(p.view(10.0).and_then(|v| offset_of(&v, 2)), Some((-32, 0)));
}

#[test]
fn a_foreign_publication_ends_every_slide_and_a_scene_of_the_same_places_lets_them_run_on() {
    let start = scene(
        (100, 100),
        0,
        &[(LEADER, (100, 100), 0x180), (2, (96, 95), 0x181)],
    );
    let moved = scene(
        (101, 100),
        0,
        &[(LEADER, (101, 100), 0x180), (2, (97, 95), 0x181)],
    );
    let mut p = two(start.clone(), moved.clone(), 0.0);
    // An idle refresh (an animation, the same places): the slides run on.
    p.publish(Rc::new(moved.clone()), 100.0, true);
    let v = p.view(180.0).expect("running on");
    assert_eq!((v.camera, offset_of(&v, 2)), ((-8, 0), Some((-8, 0))));
    p.foreign();
    assert!(
        p.view(180.0).is_none() && !p.active(180.0),
        "a foreign publication ends them"
    );
    // The next move slides from where the objects stand.
    let next = scene(
        (102, 100),
        0,
        &[(LEADER, (102, 100), 0x180), (2, (98, 95), 0x181)],
    );
    p.publish(Rc::new(next), 400.0, true);
    let v = p.view(400.0).expect("a new slide");
    assert_eq!((v.camera, offset_of(&v, 2)), ((-16, 0), Some((-16, 0))));
}

#[test]
fn the_leaving_strip_comes_from_the_scene_before_without_the_objects_the_new_one_draws() {
    // The leader steps east: the view's west column leaves it, drawn from the scene before.
    let start = scene(
        (100, 100),
        0,
        &[(LEADER, (100, 100), 0x180), (2, (95, 97), 0x181)],
    );
    let moved = scene(
        (101, 100),
        0,
        &[(LEADER, (101, 100), 0x180), (2, (96, 97), 0x181)],
    );
    let p = two(start, moved, 0.0);
    let plan = p.plan(&p.view(0.0).expect("a slide"));
    let bases = |scene: usize| {
        plan.iter()
            .filter(|op| matches!(op.draw, Draw::Base { scene: s, .. } if s == scene))
            .count()
    };
    assert_eq!(bases(0), SIDE * SIDE, "the newest scene's cells");
    assert_eq!(bases(1), SIDE, "the west column, from the scene before");
    let words = plan
        .iter()
        .filter(|op| op.draw == Draw::Word(ACTOR))
        .count();
    assert_eq!(
        words, 2,
        "each actor once: the mover not again from the strip"
    );
}

#[test]
fn slides_are_shown_down_to_two_display_frames_and_never_at_max() {
    let hz60 = 1000.0 / 60.0;
    for (rate, shown) in [(1.0, true), (2.0, true), (8.0, true), (12.0, false)] {
        assert_eq!(slides_shown(Some(rate), hz60), shown, "at {rate}x on 60 Hz");
    }
    assert!(!slides_shown(None, hz60), "Max");
    assert!(slides_shown(Some(8.0), 1000.0 / 144.0));
}

#[test]
fn a_preview_slides_at_once_is_held_until_the_engine_shows_its_places_and_then_only_the_npcs_slide()
{
    let start = scene(
        (100, 100),
        0,
        &[(LEADER, (100, 100), 0x180), (2, (96, 95), 0x181)],
    );
    let early = scene(
        (101, 100),
        0,
        &[(LEADER, (101, 100), 0x180), (2, (96, 95), 0x181)],
    );
    let shown = scene(
        (101, 100),
        0,
        &[(LEADER, (101, 100), 0x180), (2, (97, 95), 0x181)],
    );
    let mut p = Presenter::new();
    p.publish(Rc::new(start.clone()), 0.0, true);
    p.preview(Rc::new(early.clone()), 1000.0, true);
    let v = p.view(1000.0).expect("the preview slides at once");
    assert_eq!(
        (v.camera, offset_of(&v, LEADER)),
        ((-16, 0), Some((-16, 0)))
    );
    // Its slide over, the preview is held at rest: the engine's frame still shows the old places.
    p.end(1500.0);
    assert!(p.active(1500.0), "held until the engine shows it");
    assert_eq!(
        p.view(1500.0).map(|v| (v.camera, v.movers.len())),
        Some(((0, 0), 0))
    );
    // The engine's picture of the turn: the party where the preview had it, an NPC moved.
    p.publish(Rc::new(shown), 1540.0, true);
    let v = p.view(1540.0).expect("the NPC slides");
    assert_eq!(
        (v.camera, offset_of(&v, LEADER), offset_of(&v, 2)),
        ((0, 0), None, Some((-16, 0)))
    );
    assert!(!p.active(1900.0), "no longer held");
    // A foreign publication ends a held preview; no time does (round 13: past the 2000 ms the
    // timer held it before, and a minute on), the command prompt does, with the slides running
    // on and the generation kept, and a snap does, a new generation; none is shown where slides
    // are not.
    let mut p = Presenter::new();
    p.publish(Rc::new(start.clone()), 0.0, true);
    p.preview(Rc::new(early.clone()), 1000.0, true);
    p.foreign();
    assert!(
        !p.active(1100.0) && p.view(1100.0).is_none(),
        "a foreign picture ends it"
    );
    let mut p = Presenter::new();
    p.publish(Rc::new(start.clone()), 0.0, true);
    p.preview(Rc::new(early.clone()), 1000.0, true);
    for t in [3000.0, 61_000.0] {
        p.end(t);
        assert!(
            p.active(t) && p.view(t).map(|v| v.camera) == Some((0, 0)),
            "held at rest {t} ms on"
        );
    }
    let generation = p.generation();
    p.prompt();
    assert!(
        !p.active(61_000.0) && p.view(61_000.0).is_none() && p.generation() == generation,
        "the prompt ends it"
    );
    let mut p = Presenter::new();
    p.publish(Rc::new(start.clone()), 0.0, true);
    p.preview(Rc::new(early.clone()), 1000.0, true);
    p.prompt();
    let v = p.view(1100.0).expect("the slide runs on past the prompt");
    assert!(v.camera != (0, 0) && p.active(1100.0) && !p.active(1360.0));
    let generation = p.generation();
    p.preview(Rc::new(early.clone()), 2000.0, true);
    p.snap();
    assert!(
        !p.active(2000.0) && p.view(2000.0).is_none() && p.generation() > generation,
        "a snap ends it"
    );
    let mut p = Presenter::new();
    p.publish(Rc::new(start), 0.0, true);
    p.preview(Rc::new(early), 1000.0, false);
    assert!(
        p.view(1000.0).is_none() && !p.active(1000.0),
        "no preview at Max"
    );
}

#[test]
fn the_layers_split_the_plan_by_how_it_moves_and_placed_they_are_the_plan() {
    // The doorway step of the occlusion test: the mover's word in its own layer, the arch over it
    // in its occluders, neither among the static words; each part placed is the plan.
    let (above, door) = ((100, 97), (100, 98));
    let build = |at| {
        let mut s = scene(
            (100, 100),
            0,
            &[(LEADER, (100, 100), 0x180), (2, at, 0x181)],
        );
        add(&mut s, door, ARCH, Some(9), Rank::Top);
        place(&mut s, 9, door, 0x10);
        s
    };
    let mut p = Presenter::new();
    let g0 = p.generation();
    p.publish(Rc::new(build(above)), 0.0, true);
    p.publish(Rc::new(build(door)), 0.0, true);
    assert!(p.generation() > g0, "a publication is a new generation");
    let layers = p.layers();
    let words = |ops: &[Op], w: u16| ops.iter().filter(|o| o.draw == Draw::Word(w)).count();
    assert_eq!(layers.movers.len(), 1);
    let m = &layers.movers[0];
    assert_eq!(
        (m.slot, words(&m.ops, ACTOR), words(&m.over, ARCH)),
        (2, 1, 1)
    );
    assert_eq!(
        (words(&layers.statics, ACTOR), words(&layers.statics, ARCH)),
        (1, 1)
    );
    for t in [0.0, 90.0, 180.0, SLIDE_MS] {
        let view = p.view(t).expect("a slide");
        let mut placed = Vec::new();
        for (part, (dx, dy)) in layers.placed(&view) {
            let ops = match part {
                Part::Static => &layers.statics,
                Part::Mover(i) => &layers.movers[i].ops,
                Part::Over(i) => &layers.movers[i].over,
            };
            placed.extend(ops.iter().map(|o| Op {
                x: o.x + dx,
                y: o.y + dy,
                ..*o
            }));
        }
        assert_eq!(placed, p.plan(&view), "at {t}");
    }
    let g = p.generation();
    p.end(SLIDE_MS);
    assert_eq!(p.generation(), g, "a slide's end keeps the generation");
    p.foreign();
    assert!(p.generation() > g, "a foreign publication is a new one");
}

#[test]
fn a_layer_tells_the_blocks_it_changed_from_another_moved_by_a_shift() {
    let mut old = LayerPixels::clear(-16, -16, 64, 48);
    for (i, p) in old.pixels.iter_mut().enumerate() {
        *p = (i % 64 / 16 + 4 * (i / 64 / 16)) as u8;
    }
    old.drawn.fill(true);
    assert_eq!(
        old.changed_since(&old, (0, 0)),
        Some(vec![]),
        "the same: nothing"
    );
    let mut new = old.clone();
    new.pixels[20 * 64 + 40] = 99;
    assert_eq!(
        new.changed_since(&old, (0, 0)),
        Some(vec![[32, 16, 16, 16]]),
        "one block"
    );
    // Moved one block west: each block shows its east neighbour's old pixels; the east column has
    // none and is dirty, a block of other pixels too.
    let mut moved = LayerPixels::clear(-16, -16, 64, 48);
    moved.drawn.fill(true);
    for i in 0..64 * 48 {
        let (x, y) = (i % 64, i / 64);
        moved.pixels[i] = if x + 16 < 64 {
            old.pixels[y * 64 + x + 16]
        } else {
            7
        };
    }
    moved.pixels[40 * 64] = 98;
    assert_eq!(
        moved.changed_since(&old, (-16, 0)),
        Some(vec![[48, 0, 16, 48], [0, 32, 16, 16]]),
        "the east column one rect"
    );
    assert_eq!(
        moved.changed_since(&LayerPixels::clear(0, 0, 64, 48), (0, 0)),
        None,
        "elsewhere"
    );
    let r = moved.rect([16, 16, 32, 16]);
    assert_eq!((r.x, r.y, r.width, r.height), (0, 0, 32, 16));
    assert_eq!(r.pixels[0], moved.pixels[16 * 64 + 16]);
}

/// The blocks a layer changed, told pixel by pixel: block (bx, by) of `new` differs when a pixel
/// of it has no pixel of `old` at its place less `shift`, or another drawn bit, or (drawn) another
/// colour or source.
fn dirty_blocks(
    new: &LayerPixels,
    old: &LayerPixels,
    shift: (i32, i32),
) -> BTreeSet<(usize, usize)> {
    let mut out = BTreeSet::new();
    for y in 0..new.height {
        for x in 0..new.width {
            let (ox, oy) = (x as i32 - shift.0, y as i32 - shift.1);
            let i = y * new.width + x;
            let same =
                (0..old.width as i32).contains(&ox) && (0..old.height as i32).contains(&oy) && {
                    let o = oy as usize * old.width + ox as usize;
                    new.drawn[i] == old.drawn[o]
                        && (!new.drawn[i]
                            || new.pixels[i] == old.pixels[o] && new.src[i] == old.src[o])
                };
            if !same {
                out.insert((x / 16, y / 16));
            }
        }
    }
    out
}

/// The blocks rects cover, each block once (none covered twice), the rects inside the layer.
fn covered(rects: &[[usize; 4]], layer: &LayerPixels) -> BTreeSet<(usize, usize)> {
    let mut out = BTreeSet::new();
    for &[x, y, w, h] in rects {
        assert!(
            x % 16 == 0 && y % 16 == 0 && w > 0 && h > 0,
            "{rects:?}: on the blocks"
        );
        assert!(
            x + w <= layer.width && y + h <= layer.height,
            "{rects:?}: inside"
        );
        for by in y / 16..(y + h).div_ceil(16) {
            for bx in x / 16..(x + w).div_ceil(16) {
                assert!(out.insert((bx, by)), "{rects:?}: block ({bx}, {by}) twice");
            }
        }
    }
    out
}

#[test]
fn a_layers_changes_are_the_blocks_that_differ_at_any_shift_edge_blocks_and_bits_included() {
    // 70 × 45: a partial column and row of blocks at the east and south edges.
    let (w, h) = (70, 45);
    let mut seed = 1u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        seed >> 16
    };
    let mut old = LayerPixels::clear(-16, -16, w, h);
    for i in 0..w * h {
        old.pixels[i] = (next() % 4) as u8;
        old.drawn[i] = next() % 8 != 0;
        old.src[i].id = (next() % 3) as u16;
    }
    let at = |x: i32, y: i32| {
        ((0..w as i32).contains(&x) && (0..h as i32).contains(&y))
            .then(|| y as usize * w + x as usize)
    };
    let shifts = [
        (0, 0),
        (0, -16),
        (0, 32),
        (-16, 16),
        (16, -16),
        (-5, 3),
        (7, 0),
        (0, -9),
    ];
    let mut seen = [0usize; 4];
    for shift in shifts {
        // The old pixels moved by the shift, then changes of each kind in a few places.
        let mut new = LayerPixels::clear(-16, -16, w, h);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let i = at(x, y).unwrap();
                if let Some(o) = at(x - shift.0, y - shift.1) {
                    new.pixels[i] = old.pixels[o];
                    new.drawn[i] = old.drawn[o];
                    new.src[i] = old.src[o];
                }
            }
        }
        for _ in 0..6 {
            let i = next() as usize % (w * h);
            match next() % 4 {
                0 => new.pixels[i] ^= 8,
                1 => new.drawn[i] = !new.drawn[i],
                2 => new.src[i].id ^= 4,
                // A clear pixel's colour and source are no one's.
                _ if !new.drawn[i] => {
                    new.pixels[i] ^= 16;
                    new.src[i].id ^= 2;
                }
                _ => {}
            }
        }
        let want = dirty_blocks(&new, &old, shift);
        let rects = new.changed_since(&old, shift).unwrap();
        assert_eq!(covered(&rects, &new), want, "shift {shift:?}: {rects:?}");
        seen[0] += usize::from(want.iter().any(|&(bx, _)| bx == w / 16));
        seen[1] += usize::from(want.iter().any(|&(_, by)| by == h / 16));
        seen[2] += usize::from(rects.iter().any(|r| r[3] > 16));
        seen[3] += usize::from(want.len() < w.div_ceil(16) * h.div_ceil(16));
    }
    assert!(
        seen.iter().all(|&n| n > 0),
        "edge blocks, merged rects and clean blocks seen: {seen:?}"
    );
    // Each kind alone: one pixel's colour, drawn bit or source changes its block, as does nothing
    // when the pixel is clear in both.
    for kind in 0..4 {
        let i = 20 * w + 37;
        let mut base = old.clone();
        base.drawn[i] = kind != 3;
        let mut new = base.clone();
        match kind {
            0 => new.pixels[i] ^= 8,
            1 => new.drawn[i] = false,
            2 => new.src[i].id ^= 4,
            _ => {
                new.pixels[i] ^= 8;
                new.src[i].id ^= 4;
            }
        }
        let want = if kind == 3 {
            vec![]
        } else {
            vec![[32, 16, 16, 16]]
        };
        assert_eq!(new.changed_since(&base, (0, 0)), Some(want), "kind {kind}");
    }
}

#[test]
fn a_restarted_slide_stays_under_the_top_tiles_of_every_cell_it_sweeps() {
    // From 96 to 97, restarted half way (drawn at 96.5) toward 98: the mover overlaps cell 96 at
    // the restart, two cells from its new cell; a top tile there covers it.
    let at = |x: i32| {
        let mut s = scene(
            (100, 100),
            0,
            &[(LEADER, (100, 100), 0x180), (2, (x, 95), 0x181)],
        );
        add(&mut s, (96, 95), WALL, Some(10), Rank::Top);
        place(&mut s, 10, (96, 95), 0x20);
        s
    };
    let mut p = two(at(96), at(97), 0.0);
    p.publish(Rc::new(at(98)), 180.0, true);
    let s = at(98);
    let fb = frame_at(&p, 180.0);
    let px = |cell, col: i32| {
        let (x, y) = s.pixel(cell);
        fb.get((x + col) as usize, (y + 8) as usize)
    };
    assert_eq!(
        px((96, 95), 12),
        Some(4),
        "under the top tile of the cell it left"
    );
    assert_eq!(px((97, 95), 4), Some(BODY), "drawn in the next cell");
}

#[test]
fn the_layers_generation_moves_only_when_what_slides_changes() {
    let at = |c: i32, npc: i32| {
        scene(
            (c, 100),
            0,
            &[(LEADER, (c, 100), 0x180), (2, (npc, 95), 0x181)],
        )
    };
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100, 96)), 0.0, true);
    let g = p.generation();
    p.publish(Rc::new(at(101, 96)), 10.0, true);
    assert!(p.generation() > g, "the camera starts sliding");
    let g = p.generation();
    p.publish(Rc::new(at(101, 96)), 50.0, true);
    assert_eq!(
        p.generation(),
        g,
        "the same places: the slides run on, the layers kept"
    );
    p.publish(Rc::new(at(101, 97)), 90.0, true);
    assert!(p.generation() > g, "a mover joins");
    let g = p.generation();
    p.publish(Rc::new(at(101, 97)), 120.0, true);
    assert_eq!(p.generation(), g);
    p.publish(Rc::new(at(102, 97)), 200.0, true);
    assert!(p.generation() > g, "the camera restarts");
    let g = p.generation();
    p.publish(Rc::new(at(110, 107)), 210.0, true);
    assert!(p.generation() > g, "a snap restarts the presenter");
    let g = p.generation();
    p.foreign();
    assert!(p.generation() > g, "a foreign picture");
}

/// Spec 20 Design 5, against an oracle of its own: with the leader stepping east (the camera
/// slides), movers 2 and 3 crossing rows going south and north, and 4 and 5 swapping cells along a
/// row, at each eighth of the slide every mover's layer is placed where the linear move of its cell
/// puts it on the screen (its world pixel less the camera's, the view's corner at the camera), the
/// movers in the order of those places (y, then x), the static words and the words over each at
/// the camera's offset.
#[test]
fn the_movers_are_placed_where_their_cells_put_them_crossing_under_a_sliding_camera() {
    let objects = |leader: i32, moved: bool| {
        let (two, three) = if moved { (100, 99) } else { (99, 100) };
        let (four, five) = if moved { (98, 97) } else { (97, 98) };
        [
            (LEADER, (leader, 100), 0x180),
            (2, (98, two), 0x181),
            (3, (99, three), 0x182),
            (4, (four, 102), 0x183),
            (5, (five, 102), 0x184),
        ]
    };
    let a = scene((100, 100), 0, &objects(100, false));
    let b = scene((101, 100), 0, &objects(101, true));
    let p = two(a.clone(), b.clone(), 0.0);
    let layers = p.layers();
    let half = (SIDE / 2) as i32;
    // Per crossing pair: seen in the first order before the crossing, in the other after it.
    let mut crossed = [[false; 2]; 2];
    for k in 0..=8 {
        let f = f64::from(k) / 8.0;
        let at = |from: i32, to: i32| f64::from(16 * from) + f * f64::from(16 * (to - from));
        let cam = (at(a.centre.0, b.centre.0), at(a.centre.1, b.centre.1));
        let view = p.view(SLIDE_MS * f).expect("a slide");
        let want_cam = (
            (cam.0 - f64::from(16 * b.centre.0)) as i32,
            (cam.1 - f64::from(16 * b.centre.1)) as i32,
        );
        assert_eq!(view.camera, want_cam, "at {k}/8: the camera");
        // Each mover's screen pixel by the oracle: its world pixel less the camera's, from the
        // view's corner at the camera.
        let mut want = BTreeMap::new();
        for slot in [LEADER, 2, 3, 4, 5] {
            let (from, to) = (a.find(slot).unwrap().cell, b.find(slot).unwrap().cell);
            let w = (at(from.0, to.0), at(from.1, to.1));
            let screen = (
                b.corner.0 + (w.0 - cam.0) as i32 + 16 * half,
                b.corner.1 + (w.1 - cam.1) as i32 + 16 * half,
            );
            want.insert(slot, screen);
        }
        let placed = layers.placed(&view);
        let n = layers.movers.len();
        assert_eq!(
            placed.len(),
            1 + 2 * n,
            "at {k}/8: the static part, the movers, the overs"
        );
        assert_eq!(placed[0], (Part::Static, (-view.camera.0, -view.camera.1)));
        let mut got = BTreeMap::new();
        let mut order = Vec::new();
        for &(part, (dx, dy)) in &placed[1..=n] {
            let Part::Mover(i) = part else {
                panic!("at {k}/8: a mover's part");
            };
            let m = &layers.movers[i];
            let screen = (m.home.0 + dx, m.home.1 + dy);
            got.insert(m.slot, screen);
            order.push((screen.1, screen.0));
        }
        assert_eq!(got, want, "at {k}/8: the movers' places");
        assert!(
            order.is_sorted(),
            "at {k}/8: drawn by their places: {order:?}"
        );
        for (j, &(part, d)) in placed[n + 1..].iter().enumerate() {
            let Part::Mover(i) = placed[1 + j].0 else {
                unreachable!()
            };
            assert_eq!(
                (part, d),
                (Part::Over(i), (-view.camera.0, -view.camera.1)),
                "at {k}/8"
            );
        }
        // 2 above 3 before the crossing, below after; 4 west of 5 before, east after.
        let index =
            |slot: u16| {
                (placed[1..=n].iter())
            .position(|&(part, _)| matches!(part, Part::Mover(i) if layers.movers[i].slot == slot))
            .unwrap()
            };
        for (pair, (a, b)) in crossed.iter_mut().zip([(2, 3), (4, 5)]) {
            pair[0] |= k < 4 && index(a) < index(b);
            pair[1] |= k > 4 && index(a) > index(b);
        }
    }
    assert!(
        crossed == [[true, true], [true, true]],
        "each crossing: one order before it, the other after: {crossed:?}"
    );
}

/// Spec 20 Design 5: a row clear in both layers is unchanged whatever colours and sources its
/// clear pixels hold (the cursor and border layer is mostly such rows) and its pixels are not
/// compared at all, and a drawn pixel's change in the same block still changes it.
#[test]
fn a_row_clear_in_both_layers_is_unchanged_and_a_drawn_change_in_its_block_counts() {
    let mut old = LayerPixels::clear(0, 0, 48, 32);
    for i in 0..48 * 32 {
        old.pixels[i] = (i % 7) as u8;
        old.src[i].id = (i % 5) as u16;
    }
    // Block (1, 0): rows 0..8 drawn, rows 8..16 clear in both.
    for y in 0..8 {
        for x in 16..32 {
            old.drawn[y * 48 + x] = true;
        }
    }
    let mut new = old.clone();
    for y in 8..16 {
        for x in 0..48 {
            new.pixels[y * 48 + x] ^= 0x20;
            new.src[y * 48 + x].id ^= 0x40;
        }
    }
    ROWS_COMPARED.with(|n| n.set(0));
    assert_eq!(
        new.changed_since(&old, (0, 0)),
        Some(vec![]),
        "clear rows: unchanged"
    );
    // Only the 8 drawn rows of block (1, 0) are compared, none of the 88 clear block rows.
    assert_eq!(
        ROWS_COMPARED.with(|n| n.get()),
        8,
        "clear rows: not compared"
    );
    new.pixels[3 * 48 + 20] ^= 1;
    assert_eq!(
        new.changed_since(&old, (0, 0)),
        Some(vec![[16, 0, 16, 16]]),
        "a drawn pixel of the block"
    );
}

/// `layers` placed for `view` as [`Presenter::plan`] places its own.
fn placed_ops(layers: &Layers, view: &View) -> Vec<Op> {
    let mut ops = Vec::new();
    for (part, (dx, dy)) in layers.placed(view) {
        let from = match part {
            Part::Static => &layers.statics,
            Part::Mover(i) => &layers.movers[i].ops,
            Part::Over(i) => &layers.movers[i].over,
        };
        ops.extend(from.iter().map(|op| Op {
            x: op.x + dx,
            y: op.y + dy,
            ..*op
        }));
    }
    ops
}

/// Spec 20 Design 5: the layers of a generation stay right when slides end at other times. The
/// leader (the camera) and mover 2 step at 0; mover 3 steps at 200 (a new generation, the others
/// running on); at 360 the first two end (dropped as the platform drops them each frame) while 3
/// slides on, and an unchanged publication at 400 keeps the generation. The layers made at 200,
/// placed for the slides at every sampled time from 200 to 560, paint what the plan split afresh
/// paints.
#[test]
fn layers_kept_while_some_slides_end_paint_what_a_fresh_split_paints() {
    let at = |leader: i32, two: i32, three: i32| {
        scene(
            (leader, 100),
            0,
            &[
                (LEADER, (leader, 100), 0x180),
                (2, (two, 102), 0x181),
                (3, (three, 98), 0x182),
            ],
        )
    };
    let mut p = two(at(100, 97, 103), at(101, 98, 103), 0.0);
    p.publish(Rc::new(at(101, 98, 104)), 200.0, true);
    let (kept, g) = (p.layers(), p.generation());
    assert_eq!(
        kept.movers.len(),
        3,
        "three movers in the layers made at 200"
    );
    let mut sampled = 0;
    for k in 0..=36 {
        let t = 200.0 + 10.0 * f64::from(k);
        if k == 20 {
            p.publish(Rc::new(at(101, 98, 104)), t, true);
            assert_eq!(
                p.generation(),
                g,
                "an unchanged publication keeps the generation"
            );
        }
        let Some(view) = p.view(t) else { continue };
        if t > 360.0 {
            assert_eq!(view.movers.len(), 1, "at {t}: mover 3 alone slides on");
        }
        let fresh = painted(&p.plan(&view));
        assert!(
            painted(&placed_ops(&kept, &view)).pixels() == fresh.pixels(),
            "at {t}: the kept layers paint the fresh split"
        );
        sampled += 1;
        // As the platform does each display frame: the slides ended by now dropped.
        p.end(t);
    }
    assert!(sampled > 30, "sampled through the staggered ends");
}

/// Spec 20 Design 3: a restart's occluders are every cell its draws cover from where it is drawn
/// to its new cell, as whole pixels: a top tile there stays over it at the restart. A reverse
/// (half way from 96 to 97, back to 96), a diagonal (half way from (96, 95) to (97, 96), on to
/// (98, 97): the cell (96, 96) off the way's last cells), and a start a fraction of a pixel past a
/// cell (from 97 toward 96, at 31/32 of the way, on to 95: drawn a pixel into cell 97).
#[test]
fn a_restart_stays_under_the_top_tiles_of_its_swept_cells_reversed_diagonal_and_fractional() {
    let case = |cells: [(i32, i32); 3], at: f64, wall: (i32, i32)| {
        let s = |cell: (i32, i32)| {
            let mut s = scene(
                (100, 100),
                0,
                &[(LEADER, (100, 100), 0x180), (2, cell, 0x181)],
            );
            add(&mut s, wall, WALL, Some(10), Rank::Top);
            place(&mut s, 10, wall, 0x20);
            s
        };
        let mut p = two(s(cells[0]), s(cells[1]), 0.0);
        p.publish(Rc::new(s(cells[2])), at, true);
        assert!(p.view(at).is_some_and(|v| v.movers.len() == 1), "restarted");
        (frame_at(&p, at), s(cells[2]))
    };
    let px = |fb: &Framebuffer, s: &Scene, cell, (col, row): (i32, i32)| {
        let (x, y) = s.pixel(cell);
        fb.get((x + col) as usize, (y + row) as usize)
    };
    // Reverse: drawn at 96.5, so over cell 97's left half, under its top tile.
    let (fb, s) = case([(96, 95), (97, 95), (96, 95)], 180.0, (97, 95));
    assert_eq!(
        px(&fb, &s, (97, 95), (4, 8)),
        Some(4),
        "reverse: under the tile"
    );
    assert_eq!(px(&fb, &s, (96, 95), (12, 8)), Some(BODY), "reverse: drawn");
    // Diagonal: drawn at (96.5, 95.5), over cell (96, 96)'s top right.
    let (fb, s) = case([(96, 95), (97, 96), (98, 97)], 180.0, (96, 96));
    assert_eq!(
        px(&fb, &s, (96, 96), (12, 4)),
        Some(4),
        "diagonal: under the tile"
    );
    assert_eq!(px(&fb, &s, (97, 96), (4, 4)), Some(BODY), "diagonal: drawn");
    // Fractional: drawn at 96 + 1/32 of a cell, rounded a whole pixel into cell 97.
    let (fb, s) = case(
        [(97, 95), (96, 95), (95, 95)],
        SLIDE_MS * 31.0 / 32.0,
        (97, 95),
    );
    assert_eq!(
        px(&fb, &s, (97, 95), (0, 8)),
        Some(4),
        "fractional: under the tile"
    );
    assert_eq!(
        px(&fb, &s, (96, 95), (8, 8)),
        Some(BODY),
        "fractional: drawn"
    );
}

/// Spec 20 Design 3, against a composition of its own: in each of the four directions, a mover
/// sliding from cell A to B restarted at 31/32 of the way toward C (drawn a pixel into A, as
/// `Track::offset` rounds), with a top tile in A and in C. At the restart and half and three
/// quarters of the way on, the three cells are the ground, the mover at its linear place rounded
/// to whole pixels, and the top tiles over both.
#[test]
fn a_fractional_restart_in_every_direction_paints_the_cells_it_sweeps_as_composed_apart() {
    let mut failed = Vec::new();
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        // A, B, C along the way, off the leader's cell.
        let a = if dx != 0 {
            (97 - dx, 95)
        } else {
            (97, 97 - dy)
        };
        let at = |k: i32| (a.0 + k * dx, a.1 + k * dy);
        let (b, c) = (at(1), at(2));
        let s = |cell: (i32, i32)| {
            let mut s = scene(
                (100, 100),
                0,
                &[(LEADER, (100, 100), 0x180), (2, cell, 0x181)],
            );
            for (k, wall) in [(10, a), (11, c)] {
                add(&mut s, wall, WALL, Some(k), Rank::Top);
                place(&mut s, k, wall, 0x20);
            }
            s
        };
        let restart = SLIDE_MS * 31.0 / 32.0;
        let mut p = two(s(a), s(b), 0.0);
        p.publish(Rc::new(s(c)), restart, true);
        let rest = s(c);
        let world = |cell: (i32, i32)| (f64::from(16 * cell.0), f64::from(16 * cell.1));
        // Where the restart starts: the first slide's place then.
        let from = {
            let (pa, pb) = (world(a), world(b));
            (
                pa.0 + (pb.0 - pa.0) * 31.0 / 32.0,
                pa.1 + (pb.1 - pa.1) * 31.0 / 32.0,
            )
        };
        for q in [0.0, 0.5, 0.75] {
            let t = restart + SLIDE_MS * q;
            let pc = world(c);
            let x = (from.0 + (pc.0 - from.0) * q, from.1 + (pc.1 - from.1) * q);
            let (ox, oy) = ((x.0 - pc.0).round() as i32, (x.1 - pc.1).round() as i32);
            let (cx, cy) = rest.pixel(c);
            let mover = (cx + ox, cy + oy);
            let fb = frame_at(&p, t);
            let mut differ = Vec::new();
            for cell in [a, b, c] {
                let (x0, y0) = rest.pixel(cell);
                for y in y0..y0 + CELL {
                    for x in x0..x0 + CELL {
                        let inside = |(mx, my): (i32, i32)| {
                            (mx..mx + CELL).contains(&x) && (my..my + CELL).contains(&y)
                        };
                        // The top tiles (whole cells) over the mover, the mover over the ground.
                        let want = if cell == a || cell == c {
                            4
                        } else if inside(mover) {
                            BODY
                        } else {
                            BASE
                        };
                        if fb.get(x as usize, y as usize) != Some(want) {
                            differ.push((x, y));
                        }
                    }
                }
            }
            if !differ.is_empty() {
                failed.push(((dx, dy), q, differ.len()));
            }
            if q == 0.0 {
                // The pixel into A is the top tile's, not the mover's.
                let (axp, ayp) = rest.pixel(a);
                let into = (mover.0 - axp, mover.1 - ayp);
                assert!(
                    (into.0.abs() == 15 || into.1.abs() == 15) && (into.0 == 0 || into.1 == 0),
                    "({dx}, {dy}): drawn a pixel into A ({into:?})"
                );
            }
        }
    }
    assert!(
        failed.is_empty(),
        "(direction, progress of the restart, pixels differing): {failed:?}"
    );
}

/// W3: only a logical teleport snaps; drawn lag never does. Random publications include
/// direction changes and idle refreshes, checked at the exact publication time.
#[test]
fn one_cell_publications_are_continuous_even_after_many_retargets() {
    let at = |x, y| scene((x, y), 0, &[(LEADER, (x, y), 0x180)]);
    let mut p = Presenter::new();
    let (mut x, mut y, mut t) = (100, 100, 0.0);
    p.publish(Rc::new(at(x, y)), t, true);
    let mut seed = 7u32;
    for _ in 0..300 {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        t += f64::from((seed >> 16) % 400);
        let (dx, dy) = [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, -1),
            (2, 0),
            (0, 0),
        ][(seed % 8) as usize];
        let before = p.camera.map_or(px((x, y)), |c| c.at(t));
        x += dx;
        y += dy;
        p.publish(Rc::new(at(x, y)), t, true);
        if dx == 2 {
            assert!(p.camera.is_none(), "logical teleport snaps");
        } else if dx != 0 || dy != 0 {
            assert_eq!(
                p.camera.unwrap().from,
                before,
                "one-cell retarget is continuous"
            );
        }
    }
}

/// Spec 20 (no slide crosses a load): an early picture that snaps (the first scene after a load)
/// restarts the presenter even where slides are not shown (Max, or too few display frames), as a
/// publication that snaps does: the engine counts it handed over (its break ends there), so a
/// later scene one cell from the scene before it does not slide from that scene, which is gone.
#[test]
fn an_early_picture_that_snaps_restarts_the_presenter_where_slides_are_not_shown() {
    let a = scene((100, 100), 0, &[(LEADER, (100, 100), 0x180)]);
    let mut b = scene((101, 100), 0, &[(LEADER, (101, 100), 0x180)]);
    b.snap = true;
    let c = scene((101, 100), 0, &[(LEADER, (101, 100), 0x180)]);
    let mut p = Presenter::new();
    p.publish(Rc::new(a.clone()), 0.0, true);
    p.preview(Rc::new(b), 100.0, false);
    p.publish(Rc::new(c), 200.0, true);
    assert!(
        p.view(200.0)
            .is_none_or(|v| v.camera == (0, 0) && v.movers.is_empty()),
        "no slide from the scene before the early picture"
    );
    assert!(
        p.history().iter().all(|h| **h != a),
        "the scene before it is gone"
    );
}

#[test]
fn turn_speed_gives_each_new_track_its_duration_and_instant_snaps() {
    let before = scene((100, 100), 0, &[(LEADER, (100, 100), ACTOR)]);
    let after = scene((101, 100), 0, &[(LEADER, (101, 100), ACTOR)]);
    for factor in [1.0, 2.0, 4.0] {
        let mut p = Presenter::new();
        p.set_duration(SLIDE_MS / factor);
        p.publish(Rc::new(before.clone()), 0.0, true);
        p.publish(Rc::new(after.clone()), 0.0, true);
        assert!(p.active(SLIDE_MS / factor - 1.0));
        assert!(!p.active(SLIDE_MS / factor));
        assert_eq!(p.view(SLIDE_MS / factor / 2.0).unwrap().camera.0.abs(), 8);
        p.set_duration(0.0);
        assert!(!p.active(1.0), "instant removes the running slide");
        p.preview(Rc::new(before.clone()), 1.0, true);
        assert!(!p.active(1.0), "instant does not start or hold a preview");
        p.publish(Rc::new(before.clone()), 1.0, true);
        assert!(!p.active(1.0), "instant does not start another track");
    }
}

/// W3: independent actual cadence, continuous retargeting, and end-rest on release.
#[test]
fn held_cadence_tracks_each_mover_and_settles_on_release() {
    let at = |x, follower| {
        scene(
            (x, 100),
            0,
            &[(LEADER, (x, 100), 0x180), (2, (follower, 101), 0x181)],
        )
    };
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100, 99)), 0.0, true);
    p.set_walking(&[LEADER, 2], (1, 0), 0.0);
    p.publish(Rc::new(at(101, 100)), 100.0, true);
    assert_eq!(p.camera.unwrap().duration, 360.0, "first press stays a tap");
    p.publish(Rc::new(at(102, 100)), 740.0, true);
    assert_eq!(p.camera.unwrap().duration, 640.0, "own measured interval");
    p.publish(Rc::new(at(102, 101)), 900.0, true);
    assert_eq!(
        p.camera.unwrap().start,
        740.0,
        "another mover cannot restart camera"
    );
    assert_eq!(
        p.movers.iter().find(|m| m.0 == 2).unwrap().1.duration,
        800.0,
        "follower owns its interval"
    );
    assert_eq!(
        p.camera.unwrap().at(1380.0),
        px((102, 100)),
        "next target reached at stable cadence"
    );
    p.publish(Rc::new(at(103, 101)), 1380.0, true);
    assert_eq!(
        p.camera.unwrap().duration,
        640.0,
        "unrelated publication is no sample"
    );
    let before = p.camera.unwrap().at(1500.0);
    p.set_walking(&[], (0, 0), 1500.0);
    assert_eq!(p.camera.unwrap().at(1500.0), before, "release never jumps");
    assert!(
        p.camera.unwrap().duration <= 360.0,
        "release settles at base pace"
    );
    assert!(!p.active(1860.0), "all movers end at rest");
}

#[test]
fn held_long_then_short_intervals_converge_without_a_24px_snap() {
    let at = |x| scene((x, 100), 0, &[(LEADER, (x, 100), 0x180)]);
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100)), 0.0, true);
    p.set_walking(&[LEADER], (1, 0), 0.0);
    p.publish(Rc::new(at(101)), 100.0, true);
    p.publish(Rc::new(at(102)), 2500.0, true);
    let before = p.camera.unwrap().at(2650.0);
    p.publish(Rc::new(at(103)), 2650.0, true);
    let c = p.camera.expect("drawn lag is not a teleport");
    assert_eq!(c.from, before, "retarget world position continuous");
    assert!(
        c.to.0 - c.from.0 > 24.0,
        "fixture crosses old snap boundary"
    );
    assert_eq!(
        c.duration, 2400.0,
        "one previous interval buffers the first short leg"
    );
    let before = c.at(2800.0);
    p.publish(Rc::new(at(104)), 2800.0, true);
    let c = p.camera.unwrap();
    assert_eq!(c.from, before, "second retarget is continuous too");
    assert_eq!(
        c.duration, 150.0,
        "the old long interval expires after one short leg"
    );
    assert_eq!(
        c.at(2950.0),
        px((104, 100)),
        "catch-up ends at the following cadence"
    );
    assert!(
        c.to.0 - c.from.0 < 48.0,
        "two-sample buffer has less than three cells of lag"
    );
    for n in 5..=12 {
        let t = 2650.0 + f64::from(n - 3) * 150.0;
        p.publish(Rc::new(at(100 + n)), t, true);
        let c = p.camera.unwrap();
        assert_eq!(c.to.0 - c.from.0, 16.0, "no accumulating lag");
        assert_eq!(c.at(t + 150.0), c.to, "each target passed");
    }
}

#[test]
fn two_measured_intervals_buffer_quiet_alternation_without_per_cell_dwell() {
    let at = |x| Rc::new(scene((x, 100), 0, &[(LEADER, (x, 100), ACTOR)]));
    for short in [150.0, 550.0] {
        let mut p = Presenter::new();
        p.publish(at(100), 0.0, true);
        p.set_walking(&[LEADER], (1, 0), 0.0);
        let mut time = 100.0;
        p.publish(at(101), time, true);
        for n in 2..=10 {
            time += if n % 2 == 0 { short } else { 750.0 };
            let from = p.camera.map_or(px((99 + n, 100)), |c| c.at(time));
            p.publish(at(100 + n), time, true);
            let c = p.camera.unwrap();
            assert_eq!(c.from, from, "retarget position is continuous");
            if n >= 3 {
                assert_eq!(c.duration, 750.0, "only the recent two intervals");
                let next_gap = if n % 2 == 0 { 750.0 } else { short };
                assert!(
                    !c.done(time + next_gap - 0.001),
                    "no cell-end dwell before next target"
                );
                assert!(c.at(time + next_gap).0 <= c.to.0, "no overshoot");
            }
            assert_eq!(p.capacity_resets(), 0);
        }
        p.set_walking(&[], (0, 0), time + 20.0);
        assert!(!p.active(time + 800.0), "release still settles");
    }
}

#[test]
fn held_camera_sweep_has_ground_after_repeated_shortening_and_idle_publications() {
    let at = |x| scene((x, 100), 0, &[(LEADER, (x, 100), 0x180)]);
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100)), 0.0, true);
    p.set_walking(&[LEADER], (1, 0), 0.0);
    let mut t = 100.0;
    let mut retained_peak = 0;
    for (n, gap) in [0.0, 9000.0, 900.0, 150.0, 150.0].into_iter().enumerate() {
        t += gap;
        p.publish(Rc::new(at(101 + n as i32)), t, true);
        for j in 0..5 {
            p.publish(Rc::new(at(101 + n as i32)), t + j as f64, true);
        }
        retained_peak = retained_peak.max(p.history().len());
        let v = p.view(t).unwrap();
        let layers = p.layers();
        // Every pixel of the central shown map has a source base, including the left edge
        // exposed by >24px lag. The fixture's ground is synthetic; no game art is involved.
        for y in 8..SIDE as i32 * 16 - 8 {
            for x in 8..SIDE as i32 * 16 - 8 {
                assert!(
                    layers
                        .statics
                        .iter()
                        .any(|o| matches!(o.draw, Draw::Base { .. })
                            && (o.x - v.camera.0..o.x - v.camera.0 + 16).contains(&x)
                            && (o.y - v.camera.1..o.y - v.camera.1 + 16).contains(&y)),
                    "no missing ground strip"
                );
            }
        }
    }
    assert_eq!(
        retained_peak, 5,
        "five sources supply the two-interval stressed sweep"
    );
    assert_eq!(
        p.capacity_resets(),
        0,
        "normal shortening stays within the resource budget"
    );
}

#[test]
fn a_long_to_short_transition_passes_each_target_and_drops_the_old_period_after_one_leg() {
    let at = |x| Rc::new(scene((x, 100), 0, &[(LEADER, (x, 100), ACTOR)]));
    for long in [750.0, 2400.0] {
        let mut p = Presenter::new();
        p.publish(at(100), 0.0, true);
        p.set_walking(&[LEADER], (1, 0), 0.0);
        let mut time = 100.0;
        let mut previous = px((100, 100));
        let mut pending = Vec::new();
        let mut passed = 0;
        let mut max_lag = 0.0f64;
        let mut max_jump = 0.0f64;
        for n in 1..=8 {
            let from = p.camera.map_or(previous, |c| c.at(time));
            p.publish(at(100 + n), time, true);
            let c = p.camera.unwrap();
            assert_eq!(c.from, from, "publication never moves the drawn position");
            pending.push(c.to.0);
            max_lag = max_lag.max(c.to.0 - c.from.0);
            if n == 3 {
                assert_eq!(c.duration, long, "one buffered short leg");
            } else if n >= 4 {
                assert_eq!(
                    c.duration, 150.0,
                    "long interval is gone, not a high-water mark"
                );
            }
            let next = time + if n == 1 { long } else { 150.0 };
            let mut sample = time;
            while sample < next {
                let following = (sample + 10.0).min(next);
                let drawn = c.at(following);
                let delta = drawn.0 - previous.0;
                assert!(
                    delta >= 0.0 && drawn.0 <= c.to.0,
                    "no rollback or overshoot"
                );
                assert!(
                    delta <= (c.to.0 - c.from.0) / c.duration * (following - sample) + 1e-9,
                    "each sample obeys its real distance/D velocity"
                );
                pending.retain(|&target| {
                    let crossed = target >= previous.0 && target <= drawn.0;
                    passed += usize::from(crossed);
                    !crossed
                });
                if n >= 4 {
                    assert!(delta > 0.0, "no rest during steady short cadence");
                }
                max_jump = max_jump.max(delta);
                previous = drawn;
                sample = following;
            }
            assert_eq!(p.capacity_resets(), 0);
            time = next;
        }
        assert!(
            pending.is_empty() && passed == 8,
            "every previous target was traversed"
        );
        assert!(
            max_lag < 48.0 && max_jump < 3.2,
            "bounded two-sample catch-up"
        );
        p.set_walking(&[], (0, 0), time);
        assert!(!p.active(time + 360.0));
        println!(
            "W3 transition {long}->150: max lag {max_lag:.2}px, max 10ms jump {max_jump:.2}px, all 8 targets passed, resource resets=0"
        );
    }
}

#[test]
fn authoritative_step_times_are_not_late_publication_intervals() {
    let at = |x| scene((x, 100), 0, &[(LEADER, (x, 100), 0x180)]);
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100)), 0.0, true);
    p.set_walking(&[LEADER], (1, 0), 0.0);
    p.observe(-1, (100, 100), 0.0);
    p.observe(-1, (101, 100), 100.0);
    p.observe(-1, (102, 100), 740.0);
    p.publish(Rc::new(at(101)), 1300.0, true); // first step's picture was late
    assert_eq!(
        p.camera.unwrap().duration,
        360.0,
        "first tap, despite later observation"
    );
    p.publish(Rc::new(at(102)), 1340.0, true); // next allowed preview only 40ms later
    assert_eq!(
        p.camera.unwrap().duration,
        640.0,
        "actual logical interval, not 40ms publication gap"
    );
    assert!(
        p.camera.unwrap().to.0 - p.camera.unwrap().from.0 > 24.0,
        "continuous drawn lag"
    );
}

#[test]
fn first_tap_uses_current_o3_but_later_observed_cadence_is_not_scaled_twice() {
    let at = |x| scene((x, 100), 0, &[(LEADER, (x, 100), 0x180)]);
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100)), 0.0, true);
    p.set_walking(&[LEADER], (1, 0), 0.0);
    p.observe(-1, (100, 100), 0.0);
    p.observe(-1, (101, 100), 100.0);
    p.set_duration(90.0);
    p.publish(Rc::new(at(101)), 100.0, true);
    assert_eq!(p.camera.unwrap().duration, 90.0, "tap 360/4");
    p.observe(-1, (102, 100), 1300.0); // 150 wall ms at 8x
    p.publish(Rc::new(at(102)), 1300.0, true);
    assert_eq!(
        p.camera.unwrap().duration,
        1200.0,
        "already measured in engine ms"
    );
    p.set_walking(&[LEADER], (-1, 0), 1450.0);
    p.observe(-1, (102, 100), 1450.0);
    p.observe(-1, (101, 100), 1450.0);
    let from = p.camera.unwrap().at(1450.0);
    p.publish(Rc::new(at(101)), 1450.0, true);
    assert_eq!(p.camera.unwrap().from, from, "reversal stays continuous");
    assert_eq!(
        p.camera.unwrap().duration,
        90.0,
        "reversal uses tap duration"
    );
}

#[test]
fn too_short_tap_does_not_erase_the_observed_held_cadence() {
    let at = |x| scene((x, 100), 0, &[(LEADER, (x, 100), 0x180)]);
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100)), 0.0, true);
    p.set_duration(90.0);
    p.set_minimum_duration(266.0);
    p.set_walking(&[LEADER], (1, 0), 0.0);
    p.observe(-1, (100, 100), 0.0);
    p.observe(-1, (101, 100), 100.0);
    p.publish(Rc::new(at(101)), 100.0, true);
    assert!(p.camera.is_none(), "tap shorter than two frames snaps");
    p.observe(-1, (102, 100), 1300.0);
    p.publish(Rc::new(at(102)), 1300.0, true);
    assert_eq!(
        p.camera.unwrap().duration,
        1200.0,
        "held interval still animates"
    );
}

#[test]
fn a_late_followers_first_short_tap_does_not_inherit_the_cameras_stride() {
    let at = |x, follower| {
        scene(
            (x, 100),
            0,
            &[(LEADER, (x, 100), ACTOR), (4, (follower, 101), ACTOR)],
        )
    };
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100, 99)), 0.0, true);
    p.set_duration(90.0); // O3 4x
    p.set_minimum_duration(134.0); // fewer than two displayed frames at engine 4x
    p.set_walking(&[LEADER, 4], (1, 0), 0.0);
    p.observe(-1, (100, 100), 0.0);
    p.observe(4, (99, 101), 0.0);
    for n in 1..=3 {
        let time = f64::from(n) * 640.0;
        p.observe(-1, (100 + n, 100), time);
        p.publish(Rc::new(at(100 + n, 99)), time, true);
    }
    p.observe(4, (100, 101), 2000.0);
    p.publish(Rc::new(at(103, 100)), 2000.0, true);
    assert!(p.camera.is_some(), "camera has a measured held cadence");
    assert!(
        p.movers.iter().all(|(slot, _)| *slot != 4),
        "follower's own first 90ms tap snaps under two displayed frames"
    );
    p.observe(4, (101, 101), 2640.0);
    p.publish(Rc::new(at(103, 101)), 2640.0, true);
    let follower = p.movers.iter().find(|(slot, _)| *slot == 4).unwrap().1;
    assert_eq!(follower.duration, 640.0, "own second step animates");
    assert_eq!(follower.from, px((100, 101)));
}

#[test]
fn releasing_or_replacing_the_held_party_does_not_retime_an_independent_npc() {
    let at = |npc| {
        scene(
            (100, 100),
            0,
            &[(LEADER, (100, 100), ACTOR), (9, (npc, 101), ACTOR)],
        )
    };
    let run = |party: Option<&[u16]>| {
        let mut p = Presenter::new();
        p.publish(Rc::new(at(100)), -100.0, true);
        p.set_walking(&[LEADER], (1, 0), -100.0);
        p.publish(Rc::new(at(101)), 0.0, true);
        p.publish(Rc::new(at(100)), 180.0, true);
        if let Some(party) = party {
            p.set_walking(party, (1, 0), 180.0);
        }
        let npc = p.movers.iter().find(|(slot, _)| *slot == 9).unwrap().1;
        (npc, [180.0, 270.0, 360.0, 450.0, 540.0].map(|t| npc.at(t)))
    };
    assert_eq!(
        run(Some(&[])),
        run(None),
        "release leaves NPC progress unchanged"
    );
    assert_eq!(
        run(Some(&[2])),
        run(None),
        "party replacement leaves NPC progress unchanged"
    );
}

#[test]
fn a_followers_diagonal_then_straight_steps_keep_its_own_held_cadence() {
    let at = |follower| {
        scene(
            (100, 100),
            0,
            &[(LEADER, (100, 100), ACTOR), (4, follower, ACTOR)],
        )
    };
    let mut p = Presenter::new();
    p.publish(Rc::new(at((99, 100))), 0.0, true);
    p.set_duration(90.0);
    p.set_minimum_duration(134.0);
    p.set_walking(&[LEADER, 4], (0, 1), 0.0);
    p.observe(4, (99, 100), 0.0);
    p.observe(4, (100, 101), 100.0);
    p.publish(Rc::new(at((100, 101))), 100.0, true);
    assert!(p.movers.is_empty(), "only its first tap is too short");
    p.observe(4, (100, 102), 740.0);
    p.publish(Rc::new(at((100, 102))), 740.0, true);
    let m = p.movers.iter().find(|(slot, _)| *slot == 4).unwrap().1;
    assert_eq!(
        m.duration, 640.0,
        "formation direction did not reset cadence"
    );
    let before = m.at(900.0);
    p.observe(4, (101, 103), 900.0);
    p.publish(Rc::new(at((101, 103))), 900.0, true);
    let m = p.movers.iter().find(|(slot, _)| *slot == 4).unwrap().1;
    assert_eq!(m.from, before, "retarget begins at actual drawn position");
    assert_eq!(m.duration, 640.0, "one older interval is buffered");
    p.set_walking(&[LEADER, 4], (-1, 0), 920.0);
    p.observe(4, (101, 103), 920.0);
    p.observe(4, (100, 103), 940.0);
    p.publish(Rc::new(at((100, 103))), 940.0, true);
    assert!(
        p.movers.is_empty(),
        "physical input direction resets tap timing"
    );
}

#[test]
fn resource_exhaustion_is_bounded_and_resets_to_the_exact_newest_scene() {
    let at = |x| scene((x, 100), 0, &[(LEADER, (x, 100), 0x180)]);
    let mut p = Presenter::new();
    p.publish(Rc::new(at(100)), 0.0, true);
    p.set_duration(f64::MAX);
    for n in 1..=200 {
        let shot = Rc::new(at(100 + n));
        p.publish(Rc::clone(&shot), 1.0, true); // pathological one-cell burst with no elapsed time
        assert!(p.history().len() <= MAX_HISTORY, "snapshot cap");
        if let Some((_, _, w, h)) = p.static_bounds() {
            assert!(w * h <= MAX_STATIC_PIXELS, "raster cap");
        }
        if !p.active(1.0) {
            assert_eq!(p.history().len(), 1, "reset purges retained sources");
            assert!(
                Rc::ptr_eq(&p.history()[0], &shot),
                "the exact newest scene survives"
            );
        }
    }
    assert!(
        p.capacity_resets() > 0,
        "fixture exhausts the independent resource budget"
    );
    p.set_walking(&[], (0, 0), 2.0);
    p.set_duration(360.0);
    p.foreign();
    assert_eq!(
        p.history().len(),
        1,
        "foreign purges obsolete source scenes"
    );
    p.publish(Rc::new(at(400)), 3.0, true);
    assert!(
        !p.active(3.0) && p.history().len() == 1,
        "logical teleport purges old sources"
    );
    p.clear();
    assert_eq!(p.capacity_resets(), 0);
}

#[test]
fn the_snapshot_limit_resets_even_when_the_static_pixel_budget_still_fits() {
    let at = |x| {
        let mut s = scene((x, 100), 0, &[]);
        s.cols = 37;
        s.rows = 21;
        s.origin = (x - 18, 90);
        s.cells = vec![
            Cell {
                base: GROUND,
                nodes: Vec::new()
            };
            s.cols * s.rows
        ];
        Rc::new(s)
    };
    let mut p = Presenter::new();
    p.publish(at(100), 0.0, true);
    p.set_duration(f64::MAX);
    for n in 1..=16 {
        let shot = at(100 + n);
        p.publish(Rc::clone(&shot), 1.0, true);
        if n < 16 {
            assert_eq!(p.capacity_resets(), 0);
            assert_eq!(p.history().len(), n as usize + 1);
        } else {
            // 37 columns + 16 lag cells + one leading margin, by 23 rows: the area
            // fits the 45x29 view budget, but seventeen required source scenes do not.
            assert!(
                (shot.cols + n as usize + 1) * (shot.rows + 2) < (shot.cols + 8) * (shot.rows + 8)
            );
            assert_eq!(p.capacity_resets(), 1, "snapshot resource reset");
            assert_eq!(p.history().len(), 1);
            assert!(Rc::ptr_eq(&p.history()[0], &shot));
            assert!(!p.active(1.0), "exact latest picture after reset");
        }
    }
}

#[test]
fn pathological_raster_size_and_nonfinite_clocks_cannot_allocate_unbounded_layers() {
    let mut s = scene((100, 100), 0, &[]);
    s.cols = MAX_STATIC_PIXELS;
    let mut p = Presenter::new();
    p.publish(Rc::new(s.clone()), 0.0, true);
    s.centre.0 += 1;
    p.publish(Rc::new(s), 1.0, true);
    assert_eq!(p.capacity_resets(), 1);
    assert!(p.static_bounds().is_none() && p.layers().statics.is_empty());
    p.clear();
    let at = |x| Rc::new(scene((x, 100), 0, &[]));
    p.publish(at(100), 0.0, true);
    p.publish(at(101), f64::NAN, true);
    assert!(!p.active(1.0), "invalid clock uses exact scene");
}

#[test]
fn an_overflowed_or_future_observation_never_becomes_another_steps_period() {
    let at = |x| Rc::new(scene((x, 100), 0, &[(LEADER, (x, 100), 0x180)]));
    let mut p = Presenter::new();
    p.publish(at(100), 0.0, true);
    p.set_walking(&[LEADER], (1, 0), 0.0);
    p.observe(-1, (100, 100), 0.0);
    for n in 1..=8 {
        p.observe(-1, (100 + n, 100), f64::from(n) * 640.0);
    }
    p.publish(at(101), 100.0, true);
    assert_eq!(
        p.camera.unwrap().duration,
        360.0,
        "evicted old target is tap fallback"
    );
    p.publish(at(102), 140.0, true);
    assert_eq!(
        p.camera.unwrap().duration,
        360.0,
        "not the short publication interval or a future sample"
    );
    p.observe(-1, (109, 100), f64::INFINITY);
    assert!(p.observations.values().all(|o| o.periods.len() <= 4));
}

#[test]
fn route_corners_keep_measured_cadence_but_a_replacement_starts_a_new_session() {
    let mut p = Presenter::new();
    p.set_route_walking(&[LEADER], 7, 0.0);
    let start = (100, 100);
    p.observe(-1, start, 0.0);
    p.publish(
        Rc::new(scene(start, 0, &[(LEADER, start, ACTOR)])),
        0.0,
        true,
    );
    for (i, cell) in [(101, 100), (101, 101), (100, 101)].into_iter().enumerate() {
        let t = (i + 1) as f64 * 660.0;
        p.set_route_walking(&[LEADER], 7, t);
        p.observe(-1, cell, t);
        p.publish(Rc::new(scene(cell, 0, &[(LEADER, cell, ACTOR)])), t, true);
        assert_eq!(
            p.camera.unwrap().duration,
            if i == 0 { SLIDE_MS } else { 660.0 },
            "corners retain measured route cadence"
        );
    }
    p.set_route_walking(&[LEADER], 8, 2000.0);
    let cell = (100, 102);
    p.observe(-1, cell, 2640.0);
    p.publish(
        Rc::new(scene(cell, 0, &[(LEADER, cell, ACTOR)])),
        2640.0,
        true,
    );
    assert_eq!(
        p.camera.unwrap().duration,
        SLIDE_MS,
        "a new route does not inherit the old route cadence"
    );
    p.set_walking(&[], (0, 0), 2700.0);
    p.end(4000.0);
    assert!(!p.walking() && p.timings().is_empty());
    assert_eq!(p.capacity_resets(), 0);
    assert_eq!(p.history().len(), 1);
}

#[test]
fn a_route_track_passes_each_corner_even_when_one_display_crosses_several() {
    let mut track = Track::straight((0.0, 0.0), (16.0, 0.0), 0.0, 100.0);
    for to in [(16.0, 16.0), (0.0, 16.0), (0.0, 0.0)] {
        track = track.retarget(to, 0.0, 100.0, true).unwrap();
    }
    assert_eq!(track.length(), 64.0, "four complete Chebyshev segments");
    assert_eq!(track.at(25.0), (16.0, 0.0));
    assert_eq!(track.at(50.0), (16.0, 16.0));
    assert_eq!(track.at(75.0), (0.0, 16.0));
    assert_eq!(track.at(100.0), (0.0, 0.0));
    assert_eq!(
        track.at(200.0),
        (0.0, 0.0),
        "no overshoot after all corners"
    );
    let next = track.retarget((-16.0, 0.0), 80.0, 100.0, true).unwrap();
    assert_eq!(
        next.from,
        track.at(80.0),
        "retarget from the drawn fractional point"
    );
    assert_eq!(
        next.via[..next.via_len],
        [(0.0, 0.0)],
        "all earlier crossed corners are dropped"
    );
    assert_eq!(
        track.bounds(),
        ((0.0, 0.0), (16.0, 16.0)),
        "closed-loop sweep includes its intermediate corners"
    );
}

#[test]
fn route_waypoint_overflow_uses_the_counted_resource_reset() {
    let mut p = Presenter::new();
    p.set_route_walking(&[LEADER], 1, 0.0);
    let cells = [(100, 100), (101, 100), (101, 101), (100, 101)];
    p.publish(
        Rc::new(scene(cells[0], 0, &[(LEADER, cells[0], ACTOR)])),
        0.0,
        true,
    );
    for i in 1..=MAX_WAYPOINTS + 2 {
        let cell = cells[i % cells.len()];
        let shot = Rc::new(scene(cell, 0, &[(LEADER, cell, ACTOR)]));
        p.publish(Rc::clone(&shot), 0.0, true);
        assert!(p.history().len() <= MAX_HISTORY);
        assert!(p.camera.is_none_or(|m| m.via_len <= MAX_WAYPOINTS));
        assert!(p.movers.iter().all(|(_, m)| m.via_len <= MAX_WAYPOINTS));
        if i <= MAX_WAYPOINTS + 1 {
            assert_eq!(p.capacity_resets(), 0);
        } else {
            assert_eq!(
                p.capacity_resets(),
                1,
                "only the exhausted waypoint budget resets"
            );
            assert!(p.timings().is_empty());
            assert!(
                Rc::ptr_eq(&p.history()[0], &shot),
                "reset keeps the exact newest scene"
            );
        }
    }
}

#[test]
fn route_release_preserves_corners_but_replace_restore_and_foreign_cancel_them() {
    let make = || {
        let mut p = Presenter::new();
        p.set_route_walking(&[LEADER], 1, 0.0);
        for cell in [(100, 100), (101, 100), (101, 101)] {
            p.publish(Rc::new(scene(cell, 0, &[(LEADER, cell, ACTOR)])), 0.0, true);
        }
        assert_eq!(p.camera.unwrap().via_len, 1);
        p
    };
    let mut released = make();
    released.set_walking(&[], (0, 0), 10.0);
    assert_eq!(
        released.camera.unwrap().via_len,
        1,
        "arrival/stop finishes through the remaining corner"
    );
    released.end(2000.0);
    assert!(released.timings().is_empty() && released.history().len() == 1);
    let mut replaced = make();
    replaced.set_route_walking(&[LEADER], 2, 10.0);
    assert_eq!(
        replaced.camera.unwrap().via_len,
        0,
        "a new route cancels the previous route's pending corners"
    );
    for restore in [false, true] {
        let mut p = make();
        if restore {
            let mut restored = scene((100, 100), 0, &[(LEADER, (100, 100), ACTOR)]);
            restored.snap = true;
            p.publish(Rc::new(restored), 20.0, true);
        } else {
            p.foreign();
        }
        assert!(p.timings().is_empty() && p.history().len() == 1);
        assert_eq!(
            p.capacity_resets(),
            0,
            "explicit cancellation is not resource exhaustion"
        );
    }
}

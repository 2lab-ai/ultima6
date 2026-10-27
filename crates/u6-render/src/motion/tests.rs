//! The presenter's receipts (spec 20): the camera's slide and snap, the movers matched by slot,
//! the occlusion order as frames, the restart and the snap of a move during a slide, the end of
//! every slide at a foreign publication, and the slide's length. Synthetic scenes only.

use super::*;
use crate::Framebuffer;

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
    let mut fb = Framebuffer::new();
    let view = p.view(t).expect("a slide");
    for op in p.plan(&view) {
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
fn a_move_during_a_slide_restarts_where_the_mover_is_drawn_or_snaps_past_a_cell_and_a_half() {
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
    // At the slide's start the next step is two cells from where it is drawn: it snaps.
    let mut p = two(at(96), at(97), 0.0);
    p.publish(Rc::new(at(98)), 10.0, true);
    assert_eq!(p.view(10.0).and_then(|v| offset_of(&v, 2)), None);
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
    // A foreign publication ends a held preview; one not followed for PREVIEW_MS lapses; none is
    // shown where slides are not.
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
    assert!(p.active(1000.0 + PREVIEW_MS - 1.0) && !p.active(1000.0 + PREVIEW_MS));
    let mut p = Presenter::new();
    p.publish(Rc::new(start), 0.0, true);
    p.preview(Rc::new(early), 1000.0, false);
    assert!(
        p.view(1000.0).is_none() && !p.active(1000.0),
        "no preview at Max"
    );
}

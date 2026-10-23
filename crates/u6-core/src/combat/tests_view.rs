//! Spec 13 on the combat code's pictures, in both builds: a missile is drawn exactly while it is
//! in the shown map, the map overview is the window at 4 pixels a cell centred in the shown map
//! with the leader's cell marked, and on the surface the overview leaves out the cells past the
//! world's edge from the leader. Synthetic states, no game data.

use std::cell::Cell;
use std::rc::Rc;

use u6_formats::position::Position;

use super::peer;
use super::present::{self, flight};
use super::tables::CombatTables;
use crate::Files;
use crate::engine::Engine;
use crate::geom::{HALF_H, HALF_W, LEAD, MAP_X, MAP_Y, SHOWN, WINDOW};
use crate::state::State;

/// The view centre of these tests.
const C: Position = Position {
    x: 100,
    y: 100,
    z: 0,
};

/// A missile is drawn while its pixel lies in the shown map, the view less half a cell each side
/// (spec 13 Decision 5): on the view's edge columns and rows (the half cell next to the centre's
/// side) it is, one pixel further out it is not; the centre cell lands at the map's corner plus
/// its view cell's pixels (C3).
#[test]
fn a_missile_at_the_view_s_edge_is_drawn_and_one_past_it_is_not() {
    let (cx, cy) = (C.x as i16 * 16, C.y as i16 * 16);
    let (hw, hh) = (HALF_W as i16 * 16, HALF_H as i16 * 16);
    // The first and last map pixels on screen, per axis.
    let (left, right) = (cx - hw + 8, cx + hw + 7);
    let (top, bottom) = (cy - hh + 8, cy + hh + 7);
    let (mx, my) = (cx + 8, cy + 8);
    for (px, py, drawn) in [
        (left, my, true),
        (left - 1, my, false),
        (right, my, true),
        (right + 1, my, false),
        (mx, top, true),
        (mx, top - 1, false),
        (mx, bottom, true),
        (mx, bottom + 1, false),
    ] {
        let got = present::on_screen(C, px, py);
        assert_eq!(
            got.is_some(),
            drawn,
            "({px}, {py}) from the centre's pixel ({cx}, {cy})"
        );
    }
    let (x0, y0, x1, y1) = SHOWN;
    assert_eq!(
        present::on_screen(C, left, top),
        Some((x0 as i16, y0 as i16))
    );
    assert_eq!(
        present::on_screen(C, right, bottom),
        Some((x1 as i16, y1 as i16))
    );
    let corner = (MAP_X as i16 + hw, MAP_Y as i16 + hh);
    assert_eq!(
        present::on_screen(C, cx, cy),
        Some(corner),
        "the centre cell"
    );
}

/// A state centred on `c` with the distance table's shape (the larger of |dx| and |dy|), no
/// shooter, and the object lists linked.
fn flight_world(c: Position) -> State {
    let mut s = State::empty();
    s.map.centre = c;
    let distance = (0..64).map(|i| (i / 8).max(i % 8) as u8).collect();
    s.combat.tables = CombatTables {
        distance,
        ..CombatTables::default()
    };
    s.combat.shooter = -1;
    crate::main_loop::test_support::link_placed_objects(&mut s);
    s
}

/// The frames a one-missile trail flight of [`flight_world`] from `from` to `to` records for its
/// drawn steps (the step's changed rectangle on the screen).
fn drawn_steps(from: (i16, i16), to: (i16, i16)) -> usize {
    let out = Rc::new(Cell::new(None));
    let keep = Rc::clone(&out);
    let mut e = Engine::with_game(
        flight_world(C),
        Files::default(),
        move |st, _io| async move {
            let mut s = st.borrow_mut();
            let (mut xs, mut ys) = ([to.0], [to.1]);
            present::fly(
                &mut s,
                from.0,
                from.1,
                &mut xs,
                &mut ys,
                1,
                7,
                0x100,
                flight::TRAIL,
            );
            let steps = s.combat.effects.iter();
            let n = steps
                .filter(|f| f.cost == present::FLIGHT_STEP_MS && f.w > 0)
                .count();
            keep.set(Some(n));
        },
    );
    e.tick(&[], 16);
    out.get().expect("the flight ran")
}

/// A flight from the view's last column: inward its first step is drawn (on the shown map's last
/// pixel column), outward none is (one pixel past it, and further).
#[test]
fn a_flight_from_the_view_s_edge_is_drawn_inward_only() {
    let (x, y) = (C.x as i16 + HALF_W as i16, C.y as i16 + 3);
    let inward = drawn_steps((x, y), (x - 3, y));
    let outward = drawn_steps((x, y), (x + 3, y));
    println!("from the edge: {inward} steps drawn inward, {outward} outward");
    assert!(inward > 0, "inward: drawn");
    assert_eq!(outward, 0, "outward: past the view");
}

/// The map overview (0x13410a) on the surface away from the world's edge, the window's cells
/// all ground (colour 5 in a checker), into a back buffer of colour 9: the shown map is cleared,
/// the window lies at 4 pixels a cell centred in it ([`WINDOW`] cells: 160 × 160 filling it in
/// Original, 240 × 240 in Wide), the leader's cell (the view's centre, [`HALF_W`] and [`HALF_H`]
/// from the view offset) is a solid 4 × 4 of colour 15, and nothing outside the shown map is
/// written. In Wide with the view's columns and rows swapped the overview would leave the frame.
#[test]
fn the_overview_is_the_window_at_4_pixels_a_cell_centred_in_the_shown_map() {
    let mut s = State::empty();
    s.combat.tables.peer_colours = [0, 5, 0, 0];
    s.map.centre = C;
    s.map.window_origin = [0x100, 0x100];
    let lead = (LEAD as i16 + 2, LEAD as i16 + 3);
    s.map.view_offset = [lead.0 - HALF_W as i16, lead.1 - HALF_H as i16];
    let out = Rc::new(std::cell::RefCell::new(None));
    let keep = Rc::clone(&out);
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        io.draw_back(|b| b.clear(9));
        peer::draw_overview(&mut st.borrow_mut());
        *keep.borrow_mut() = Some(io.peek_back(|b| b.clone()));
    });
    e.tick(&[], 16);
    let b = out.borrow_mut().take().expect("the overview ran");
    let (x0, y0, x1, y1) = SHOWN;
    let [x0, y0, x1, y1] = [x0, y0, x1, y1].map(usize::from);
    let side = WINDOW * 4;
    let at = ((x0 + x1 + 1 - side) / 2, (y0 + y1 + 1 - side) / 2);
    assert!(at.0 >= x0 && at.1 >= y0 && at.0 + side <= x1 + 1 && at.1 + side <= y1 + 1);
    let leader = (at.0 + 4 * lead.0 as usize, at.1 + 4 * lead.1 as usize);
    let (mut ground, mut marked) = (0, 0);
    for y in 0..u6_render::HEIGHT {
        for x in 0..u6_render::WIDTH {
            let shown = (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
            let over = (at.0..at.0 + side).contains(&x) && (at.1..at.1 + side).contains(&y);
            let mark =
                (leader.0..leader.0 + 4).contains(&x) && (leader.1..leader.1 + 4).contains(&y);
            let want = match (shown, over, mark) {
                (false, _, _) => 9,
                (true, false, _) => 0,
                (true, true, true) => 15,
                (true, true, false) if ((x - at.0) % 4 + (y - at.1) % 4) % 2 == 1 => 5,
                (true, true, false) => 0,
            };
            assert_eq!(b.get(x, y), Some(want), "({x}, {y})");
            ground += usize::from(want == 5);
            marked += usize::from(want == 15);
        }
    }
    println!("overview at {at:?}: {ground} ground pixels, {marked} marked");
    assert_eq!(marked, 16, "the leader's cell, inside the frame");
    assert_eq!(
        ground,
        (WINDOW * WINDOW - 1) * 8,
        "half of every other cell"
    );
}

/// On the surface the overview keeps the window's cells on the leader's side of the world's
/// edge (x / y 0x3ff | 0): for every chunk-aligned origin and leader cell, a cell is drawn
/// exactly when it and the leader's cell lie on the same side of 0x400 ([`peer::this_side`],
/// from [`WINDOW`]). Original keeps the game's own tests (rows only, and two of the column
/// cases), which agree with it on the rows for its leader cells; Wide clips both axes.
#[test]
fn the_overview_keeps_the_cells_on_the_leader_s_side_of_the_world_s_edge() {
    let leads = LEAD as i16..=LEAD as i16 + 7;
    let past = |origin: u16, k: i16| origin as i16 + k >= 0x400;
    for origin in (0..0x400u16).step_by(8) {
        for lead in leads.clone() {
            let (from, to) = peer::this_side(origin, lead);
            for cell in 0..WINDOW as i16 {
                assert_eq!(
                    (from..to).contains(&cell),
                    past(origin, cell) == past(origin, lead),
                    "origin {origin:#x}, leader {lead}, cell {cell}"
                );
            }
            let (cols, rows) = peer::surface_cells([0x100, origin], [lead, lead]);
            assert_eq!(
                rows,
                peer::this_side(origin, lead),
                "rows, origin {origin:#x}"
            );
            assert_eq!(cols, (0, WINDOW as i16), "columns away from the edge");
        }
    }
    // Both edges at once (a corner): Wide clips both axes, Original its columns only.
    let corner = [0x3f0u16, 0x3f0];
    let lead = LEAD as i16 + 7;
    let both = (
        peer::this_side(corner[0], lead),
        peer::this_side(corner[1], lead),
    );
    let got = peer::surface_cells(corner, [lead, lead]);
    if cfg!(feature = "wide") {
        assert_eq!(got, both);
    } else {
        assert_eq!(got, ((8, WINDOW as i16), (0, WINDOW as i16)));
    }
}

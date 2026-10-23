//! Real-data checks on the user's map: the start position, the walls around it and the view
//! the leader gets. Skipped when the archive is absent; reads it, copies nothing, prints only
//! tile numbers and flag letters.

use std::path::Path;

use super::flags::*;
use super::view::DARK;
use super::{WINDOW, cell, region, view};
use crate::files::Files;
use crate::geom::{self, HALF_H, HALF_W, RULE_HALF, VIEW_H, VIEW_W};
use crate::state::State;
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

#[allow(clippy::disallowed_methods)] // tests may read files
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

/// The boot state with the view tables loaded and the region around the leader refreshed.
fn boot() -> Option<State> {
    let files = files()?;
    let mut s = State::new_game(&files).quiet();
    s.map.load_tables(&files).quiet();
    region::refresh_region(&mut s);
    Some(s)
}

/// View cells (col, row), row by row.
fn view_cells() -> impl Iterator<Item = (usize, usize)> {
    (0..VIEW_H).flat_map(|row| (0..VIEW_W).map(move |col| (col, row)))
}

/// The distance-table entry the sight flood reads for view cell (col, row): each axis laid over
/// the original's half view of 5 (spec 13 Decision 8; the cell's own where the half is 5).
fn sight_entry(col: usize, row: usize) -> (usize, usize) {
    let r = RULE_HALF as usize;
    (
        col.abs_diff(HALF_W) * r / HALF_W,
        row.abs_diff(HALF_H) * r / HALF_H,
    )
}

/// One letter per window cell: `#` wall that blocks, `+` other blocking tile, `~` water,
/// `.` open; the leader `@`.
fn tile_letter(s: &State, x: usize, y: usize) -> char {
    let tf = &s.tables.tileflags;
    let t = usize::from(s.map.window_tiles[cell(x, y)]);
    let f1 = flags1(tf, t);
    match () {
        _ if f1 & TF1_BLOCKING != 0 && f1 & TF1_WALL != 0 => '#',
        _ if f1 & TF1_BLOCKING != 0 => '+',
        _ if f1 & TF1_WATER != 0 => '~',
        _ => '.',
    }
}

#[test]
fn start_tile_is_walkable_and_walls_around_it_block() {
    let Some(s) = boot() else { return };
    let leader = s.leader_position().quiet();
    assert!(s.map.centre == leader);
    let (x, y, z) = (leader.x, leader.y, u16::from(leader.z));
    let tf = &s.tables.tileflags;
    assert!(s.map.tile_at(x.into(), y.into(), z.into()).is_some());
    assert!(s.map.is_passable(tf, x, y, z), "the leader's tile blocks");
    // The window cache agrees with the chunks everywhere.
    let [ox, oy] = s.map.window_origin;
    for wy in 0..WINDOW {
        for wx in 0..WINDOW {
            let (mx, my) = (ox + wx as u16, oy + wy as u16);
            let chunk = s.map.chunk_ref(mx, my, z).quiet();
            let want = s
                .map
                .chunks
                .tile(chunk.into(), usize::from(mx & 7), usize::from(my & 7));
            assert!(Some(s.map.window_tiles[cell(wx, wy)]) == want);
        }
    }
    // A blocking wall in the view window (around the leader). Nothing of the window is
    // printed: its tiles are the user's map.
    let [vx, vy] = s.map.view_offset.map(|v| v as usize);
    let walls = view_cells()
        .filter(|&(col, row)| tile_letter(&s, vx + col, vy + row) == '#')
        .count();
    assert!(walls > 0, "no blocking wall in the start view");
}

#[test]
fn start_view_sees_the_room_and_not_past_its_walls() {
    let Some(mut s) = boot() else { return };
    let files = files().quiet();
    let ambient = super::seams::ambient_light(&s);
    // The ambient light is objlist tail byte 0x0b (objlist 0x1BFC), read here from the raw file.
    let raw = files.get("savegame/objlist").quiet()[0x1BFC] as i8;
    assert!(ambient == raw, "the ambient light at objlist 0x1BFC");
    view::update_view(&mut s);
    // The start room and its walls fill the original's 11 × 11 view round the leader (all of
    // it in Original), so every cell of it is seen; the start is by day: the ambient light
    // reaches the farthest view cell, so each seen cell gains 4 - dist + ambient, clamped to 4.
    let t = s.map.tables.as_ref().quiet();
    let farthest = view_cells()
        .map(|(c, r)| sight_entry(c, r))
        .map(|(a, b)| t.dist[a][b])
        .max()
        .quiet();
    assert!(
        i16::from(ambient) >= i16::from(farthest),
        "the start is lit by day"
    );
    let r = RULE_HALF as usize;
    for (col, row) in view_cells() {
        let room = col.abs_diff(HALF_W) <= r && row.abs_diff(HALF_H) <= r;
        if room {
            assert!(s.map.is_visible(col, row), "({col}, {row}) hidden");
        }
        if s.map.is_visible(col, row) {
            assert!(s.map.light_at(col, row) == 4, "({col}, {row}) lit");
        }
    }
    assert_sight_is_closed(&s);
}

#[test]
fn view_west_of_the_start_sees_round_through_the_corridor_and_darkens_at_night() {
    let Some(mut s) = boot() else { return };
    // Three steps west of the start, inside the window the start loaded (its origin
    // `(x - 16) & 0x3f8` stays), so the window is rebuilt, not reloaded. Wide's window leads
    // by another 10 cells and may start a chunk farther west there: then the region is
    // refreshed, as a step into a new window refreshes it.
    let x0 = s.map.centre.x;
    s.map.centre.x -= 3;
    let same = geom::window_origin(s.map.centre.x) == geom::window_origin(x0);
    assert!(same || cfg!(feature = "wide"), "the same window");
    if same {
        region::rebuild_window(&mut s);
    } else {
        region::refresh_region(&mut s);
    }
    view::update_view(&mut s);
    assert_sight_is_closed(&s);
    // Night: ambient 0. Light falls off with distance from the leader (dist < 4 lit).
    s.actors.objlist_tail[0x0b] = 0;
    view::update_view(&mut s);
    assert!(s.map.view_tile(HALF_W, HALF_H) != DARK);
    // A seen cell at distance 4 or more gets no light (4 - dist + 0 <= 0) and is drawn dark.
    let mut dark = 0;
    for (col, row) in view_cells() {
        let (a, b) = sight_entry(col, row);
        let d = s.map.tables.as_ref().quiet().dist[a][b];
        let lit = s.map.light_at(col, row) > 0;
        if s.map.is_visible(col, row) {
            assert!(lit == (d < 4), "({col}, {row}): lit by its distance");
            if d >= 4 {
                assert!(s.map.view_tile(col, row) == DARK, "({col}, {row}) dark");
                dark += 1;
            }
        }
    }
    assert!(dark > 0, "a seen cell out of the light");
}

/// The flood's closure, checked from the tile flags alone: inside the sight area, a seen cell
/// that sight passes (no boundary tile, no window unless next to the leader in its row or
/// column, no boundary object) has every 8-neighbour in the area seen, and the leader is seen.
/// Holds for the region as loaded so far (actors are the only objects in slots).
fn assert_sight_is_closed(s: &State) {
    let [vx, vy] = s.map.view_offset.map(|v| v as i32);
    let (w, h) = (VIEW_W as i32, VIEW_H as i32);
    let (lx, ly) = (vx + HALF_W as i32, vy + HALF_H as i32);
    let tf = &s.tables.tileflags;
    let seen = |x: i32, y: i32| s.map.view_flags[cell(x as usize, y as usize)] & CELL_VISIBLE != 0;
    let passes = |x: i32, y: i32| {
        let c = cell(x as usize, y as usize);
        let f2 = flags2(tf, s.map.window_tiles[c].into());
        let near = (x == lx && (y - ly).abs() < 2) || (y == ly && (x - lx).abs() < 2);
        s.map.view_flags[c] & CELL_BOUNDARY == 0
            && (f2 & TF2_WINDOW == 0 || near)
            && f2 & TF2_BOUNDARY == 0
    };
    let area = |x: i32, y: i32| (vx - 1..=vx + w).contains(&x) && (vy - 1..=vy + h).contains(&y);
    assert!(seen(lx, ly));
    for y in vy - 1..=vy + h {
        for x in vx - 1..=vx + w {
            if !(seen(x, y) && passes(x, y)) {
                continue;
            }
            for (dx, dy) in [
                (0, -1),
                (1, -1),
                (1, 0),
                (1, 1),
                (0, 1),
                (-1, 1),
                (-1, 0),
                (-1, -1),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                assert!(
                    !area(nx, ny) || seen(nx, ny),
                    "({nx}, {ny}) next to seen ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn map_tables_load_from_the_users_game() {
    let Some(s) = boot() else { return };
    let t = s.map.tables.as_ref().quiet();
    // A distance table: 0 at the origin, symmetric, growing away from it.
    assert!(t.dist[0][0] == 0);
    for a in 0..8 {
        for b in 0..8 {
            assert!(t.dist[a][b] == t.dist[b][a]);
            if a > 0 {
                assert!(t.dist[a][b] >= t.dist[a - 1][b]);
            }
        }
    }
    // The walk visits the 8 neighbours once each and ends back next to the start.
    let (mut x, mut y) = (0i32, 0i32);
    let mut seen = Vec::new();
    for k in 0..8 {
        x += i32::from(t.walk_dx[k]);
        y += i32::from(t.walk_dy[k]);
        assert!(x.abs() == 1 || y.abs() == 1, "step {k} left the ring");
        seen.push((x, y));
    }
    seen.sort();
    seen.dedup();
    assert!(seen.len() == 8);
    // Loaded in either build: the image's steps (stride 40) re-encoded for the port's grids.
    for k in 0..8 {
        let (dx, dy) = (i32::from(t.walk_dx[k]), i32::from(t.walk_dy[k]));
        assert!(
            i32::from(t.walk_step[k]) == dy * WINDOW as i32 + dx,
            "step {k}"
        );
    }
}

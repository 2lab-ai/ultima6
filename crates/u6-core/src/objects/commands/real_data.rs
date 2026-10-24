//! Real data: the user's archive.org zip → u6-intake → `Files` → `State::new_game`, then the
//! object commands. Skips when the zip is absent; reads only, prints labels, slots and pass/fail
//! only (no game text, no types, places, counts or stats of the user's world; the seed search
//! writes the seeds it found to a file outside the repository).
#![allow(clippy::disallowed_methods)] // the test reads the zip

use std::cell::RefCell;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;

use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, VIEW_H, VIEW_W};
use crate::objects::{ACTOR_SLOTS, FIRST_TEMP_ACTOR, Slot};
use crate::state::State;
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

pub(super) fn files() -> Option<Files> {
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

/// The state the command loop starts from: load state and its tail (the region, the window
/// and its egg pass), RNG seeded with `seed` first as the engine does.
pub(super) fn started(files: &Files, seed: u16) -> State {
    let mut s = State::new_game(files).quiet();
    s.rng.seed(seed);
    crate::main_loop::load_state_tail(&mut s);
    s
}

/// The temporary actors (monsters) in use: slot, type, x, y, z.
pub(super) fn monsters(s: &State) -> Vec<(Slot, u16, u16, u16, u8)> {
    (FIRST_TEMP_ACTOR..ACTOR_SLOTS as Slot)
        .filter(|&m| s.objects.type_frame[m as usize] != 0)
        .map(|m| {
            let o = &s.objects;
            (m, o.kind(m), o.x(m), o.y(m), o.z(m))
        })
        .collect()
}

/// An egg as the test sees it.
struct Egg {
    /// Its map tile.
    at: (u16, u16),
    /// The templates inside: type, quantity (the most it hatches).
    templates: Vec<(u16, u16)>,
    /// Quantity 100: every template hatches its whole quantity.
    full: bool,
}

/// The hatched egg (type 0x14f, status 0x40 once hatched, 0x1319b2) nearest the leader.
fn hatched_egg(s: &mut State) -> Egg {
    let c = s.map.centre;
    let egg = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&o| {
            let ob = &s.objects;
            ob.kind(o) == 0x14f && ob.place(o) == 0 && ob.status(o) & 0x40 != 0 && ob.z(o) == c.z
        })
        .min_by_key(|&o| {
            let ob = &s.objects;
            (i32::from(ob.x(o)) - i32::from(c.x)).abs()
                + (i32::from(ob.y(o)) - i32::from(c.y)).abs()
        })
        .expect("a hatched egg");
    let mut templates = Vec::new();
    let mut t = crate::objects::first_child(&mut s.objects, egg);
    while t >= 0 {
        templates.push((s.objects.kind(t), s.objects.qq_of(t) & 0xff));
        t = crate::objects::next_child(&mut s.objects);
    }
    Egg {
        at: (s.objects.x(egg), s.objects.y(egg)),
        templates,
        full: s.objects.qq_of(egg) & 0xff == 100,
    }
}

/// [`cells_of`] every monster.
#[cfg(not(feature = "wide"))]
fn cells(s: &State) -> Vec<(i16, i16)> {
    cells_of(s, &monsters(s))
}

/// View cells (column, row of the view around the centre, [`VIEW_W`] × [`VIEW_H`]) of the
/// monsters `ms`, sorted.
fn cells_of(s: &State, ms: &[(Slot, u16, u16, u16, u8)]) -> Vec<(i16, i16)> {
    let c = s.map.centre;
    let (hw, hh) = (HALF_W as i16, HALF_H as i16);
    let mut v: Vec<(i16, i16)> = ms
        .iter()
        .map(|m| (m.2 as i16 - c.x as i16 + hw, m.3 as i16 - c.y as i16 + hh))
        .collect();
    v.sort_unstable();
    v
}

/// The monsters of `ms` the egg on tile `at` hatched, in Wide: there the window offers the eggs
/// past the wide view too (spec 13 Decision 6), and every egg's hatchlings stand on its tile or
/// where the scatter put them, within 3 cells of it on each axis (0x121e88: two draws of 1..=4,
/// less 5).
#[cfg(feature = "wide")]
fn own(ms: Vec<(Slot, u16, u16, u16, u8)>, at: (u16, u16)) -> Vec<(Slot, u16, u16, u16, u8)> {
    let near = |a: u16, b: u16| (i32::from(a) - i32::from(b)).abs() <= 3;
    ms.into_iter()
        .filter(|m| near(m.2, at.0) && near(m.3, at.1))
        .collect()
}

/// The oracle's view cells of the start egg's monsters, read off an oracle frame of the
/// original's start and kept outside the repository (the user's world): this file under the
/// oracle directory, the way the saveload tests find the original's saves. The oracle is the
/// original's view: its comparisons run in Original only.
#[cfg(not(feature = "wide"))]
const ORACLE_CELLS: &str = "/Users/zhugehyuk/Games/ultima6/oracle/hatch/start-gargoyle-cells.txt";
/// Where the seed search writes the seeds it found, next to [`ORACLE_CELLS`].
#[cfg(not(feature = "wide"))]
const ORACLE_SEEDS: &str = "/Users/zhugehyuk/Games/ultima6/oracle/hatch/start-gargoyle-seeds.txt";

/// `col,row` pairs separated by white space (`#` lines are comments), sorted.
#[cfg(not(feature = "wide"))]
fn parse_cells(text: &str) -> Vec<(i16, i16)> {
    let mut v: Vec<(i16, i16)> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(str::split_whitespace)
        .map(|p| {
            let (x, y) = p.split_once(',').expect("col,row");
            (x.parse().quiet(), y.parse().quiet())
        })
        .collect();
    v.sort_unstable();
    v
}

/// The oracle's cells: `U6_ORACLE_GARGOYLE_CELLS="c,r c,r ..."` when set (an override), else
/// [`ORACLE_CELLS`] when present, else `None`.
#[cfg(not(feature = "wide"))]
fn oracle_cells() -> Option<Vec<(i16, i16)>> {
    if let Ok(text) = std::env::var("U6_ORACLE_GARGOYLE_CELLS") {
        return Some(parse_cells(&text));
    }
    std::fs::read_to_string(ORACLE_CELLS)
        .ok()
        .map(|t| parse_cells(&t))
}

#[test]
fn the_start_egg_hatches_its_gargoyles() {
    let Some(files) = files() else { return };
    let mut s = started(&files, 0);
    let ms = monsters(&s);
    let Egg {
        at,
        templates,
        full,
        ..
    } = hatched_egg(&mut s);
    // In Wide the check takes the start egg's own monsters (`own`); Original takes them all.
    #[cfg(feature = "wide")]
    let ms = own(ms, at);
    // Output: the template's index only (types, counts and places are the user's data).
    // Each template makes 1..=its quantity of its type (all of it from an egg of quantity 100),
    // gargoyles among them (0x16a / 0x16b, the types 0x1319b2 compares with).
    assert!(!ms.is_empty(), "the egg hatched monsters");
    for (k, &(kind, n)) in templates.iter().enumerate() {
        let made = ms.iter().filter(|m| m.1 == kind).count() as u16;
        assert!(made <= n, "template {k}: more made than its quantity");
        assert!(
            !full || made == n,
            "template {k}: a full egg makes the whole quantity"
        );
    }
    assert!(ms.iter().all(|m| templates.iter().any(|t| t.0 == m.1)));
    assert!(ms.iter().any(|m| matches!(m.1, 0x16a | 0x16b)));
    // The first stays on the egg's tile.
    assert!(ms.iter().any(|m| (m.2, m.3) == at), "one on the egg's tile");
    // Seed 0x1d3: the seed [`search_seeds_for_the_oracle_gargoyles`] found for the view cells an
    // oracle frame of the original's start shows (the oracle's own seed, the low word of
    // `time()`, is unknown). The test checks that they are in view on cells of their own, and
    // that they are the oracle's cells when those are at hand ([`oracle_cells`]: the file under
    // the oracle directory, or the environment override). Without them the placement check is
    // invariant-only: in view, one monster a cell.
    let s = started(&files, 0x1d3);
    #[cfg(not(feature = "wide"))]
    let got = cells(&s);
    #[cfg(feature = "wide")]
    let got = {
        let mut s = s;
        let at = hatched_egg(&mut s).at;
        cells_of(&s, &own(monsters(&s), at))
    };
    let mut distinct = got.clone();
    distinct.dedup();
    assert!(distinct.len() == got.len(), "one monster a cell");
    let (w, h) = (VIEW_W as i16, VIEW_H as i16);
    assert!(
        got.iter()
            .all(|&(x, y)| (0..w).contains(&x) && (0..h).contains(&y))
    );
    // The oracle frame is the original's view (Original only).
    #[cfg(not(feature = "wide"))]
    match oracle_cells() {
        Some(want) => assert!(got == want, "the oracle frame's cells"),
        None => eprintln!("the oracle's cells not at hand: placement checked by invariants only"),
    }
}

/// Seed search (run by hand with `--ignored`): every seed word whose start hatch puts the
/// monsters on the view cells an oracle frame shows ([`oracle_cells`]: read off the frame; the
/// cells are the user's world, so they stay out of the repo). The seeds follow from the user's
/// files too: they go to [`ORACLE_SEEDS`] (or `U6_ORACLE_SEEDS_OUT`), and the output says only
/// whether there are any and where they went.
#[cfg(not(feature = "wide"))]
#[test]
#[ignore]
fn search_seeds_for_the_oracle_gargoyles() {
    let Some(files) = files() else { return };
    let Some(want) = oracle_cells() else {
        eprintln!("skipped: the oracle's cells are not at hand");
        return;
    };
    let mut base = State::new_game(&files).quiet();
    crate::objects::init_slots(&mut base.objects);
    let mut hits = Vec::new();
    for seed in (0..=0xffffu16).filter(|w| w & 1 == 1) {
        let mut s = base.clone();
        s.rng.seed(seed);
        crate::main_loop::load_state_tail(&mut s);
        if cells(&s) == want {
            hits.push(seed);
        }
    }
    let out = std::env::var("U6_ORACLE_SEEDS_OUT").unwrap_or_else(|_| ORACLE_SEEDS.to_string());
    let text: String = hits.iter().map(|s| format!("{s:#06x}\n")).collect();
    std::fs::write(&out, text).quiet();
    eprintln!("seeds found: {}; written to {out}", !hits.is_empty());
}

/// The start state with the message scroll set up as main leaves it.
fn with_text(files: &Files) -> State {
    let mut s = started(files, 0x1d3);
    let t = s.ui.tables.clone();
    s.ui.text.main_windows(&t);
    s.ui.text.more_rows = i16::from(s.ui.text.win().height);
    s
}

/// The scroll lines so far, the one being written last.
fn lines_of(s: &State) -> Vec<String> {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let mut last: String = t
        .transcript_line
        .iter()
        .map(|&b| crate::ui::wide::latin_char(b))
        .collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| crate::ui::wide::latin_char(b)));
    lines.push(last);
    lines
}

type Command = Box<dyn FnOnce(Rc<RefCell<State>>, Rc<Io>) -> Pin<Box<dyn Future<Output = ()>>>>;

/// Runs one command handler on `s`: `keys` first, then Space whenever the game idles on a key
/// (more prompts). Returns the state after it, the new scroll lines and the stub hits.
fn run(s: State, files: &Files, keys: &[InputEvent], cmd: Command) -> (State, Vec<String>) {
    let (e, new) = run_engine(s, files, keys, cmd);
    (e.state().clone(), new)
}

/// [`run`], returning the engine (its frame) with the new scroll lines.
fn run_engine(s: State, files: &Files, keys: &[InputEvent], cmd: Command) -> (Engine, Vec<String>) {
    let before = lines_of(&s).len();
    let mut e = Engine::with_game(s, files.clone(), cmd);
    let mut keys = keys.iter().copied();
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        let input: Vec<InputEvent> = if e.io().pending_events() == 0 {
            keys.next().into_iter().collect()
        } else {
            Vec::new()
        };
        e.tick(&input, 16);
        if !e.finished() && e.io().pending_events() == 0 && keys.len() == 0 {
            e.tick(&[InputEvent::Key(Key::Space)], 16);
        }
    }
    assert!(e.finished(), "the command did not end");
    for id in e.stub_hits().keys() {
        eprintln!("   stub {id}");
    }
    let lines = lines_of(&e.state());
    let new = lines[before.saturating_sub(1).min(lines.len())..].to_vec();
    (e, new)
}

/// The frame pixel at the middle of the view cell holding map tile (x, y): the map drawn from
/// the wide map's pixel origin ([`MAP_X`], [`MAP_Y`]; spec 13 Geometry), 16 pixels a cell.
fn cell_pixel(s: &State, x: u16, y: u16) -> (i16, i16) {
    let c = s.map.centre;
    let col = x as i16 - c.x as i16 + HALF_W as i16;
    let row = y as i16 - c.y as i16 + HALF_H as i16;
    (MAP_X as i16 + col * 16 + 8, MAP_Y as i16 + row * 16 + 8)
}

/// Points the cross-hair at `slot` as a confirmed target does: selection, map target and the
/// pointer on its view cell.
fn aim(s: &mut State, slot: Slot) {
    let (x, y) = (s.objects.x(slot), s.objects.y(slot));
    let pointer = cell_pixel(s, x, y);
    let input = &mut s.ui.input;
    input.selection = slot;
    input.target = (x as i16, y as i16);
    input.pointer = pointer;
    input.target_void = false;
}

/// Puts the leader (the hero) at (x, y) with the view on it.
fn stand_at(s: &mut State, x: u16, y: u16) {
    let z = u16::from(s.map.centre.z);
    crate::objects::move_to(s, 1, x, y, z);
    s.map.centre.x = x;
    s.map.centre.y = y;
    crate::map::region::refresh_region(s);
    crate::map::view::update_view(s);
}

/// Names the step and whether it printed; the lines and their count are the game's.
fn print_lines(what: &str, lines: &[String]) {
    eprintln!("{what}: printed {}", lines.iter().any(|l| !l.is_empty()));
}

/// The map objects of `kind` that `keep` accepts, nearest the leader first (city-block
/// distance, then slot): where the user's world has them, found at run time.
fn nearest(s: &State, kind: u16, keep: impl Fn(&State, Slot) -> bool) -> Vec<Slot> {
    let c = s.map.centre;
    let mut v: Vec<(i32, Slot)> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&o| {
            let ob = &s.objects;
            ob.kind(o) == kind && ob.place(o) == 0 && ob.z(o) == c.z && keep(s, o)
        })
        .map(|o| {
            let ob = &s.objects;
            let d = (i32::from(ob.x(o)) - i32::from(c.x)).abs()
                + (i32::from(ob.y(o)) - i32::from(c.y)).abs();
            (d, o)
        })
        .collect();
    v.sort_unstable();
    v.into_iter().map(|(_, o)| o).collect()
}

/// The nearest map object of `kind` to the leader.
fn find(s: &State, kind: u16) -> Slot {
    *nearest(s, kind, |_, _| true)
        .first()
        .unwrap_or_else(|| panic!("no type {kind:#x} near the start"))
}

/// True when the leader can step onto (x, y) (map and objects, 0x11b1c0's test).
fn walkable(s: &State, x: i32, y: i32) -> bool {
    let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
        return false;
    };
    crate::actors::walk::can_move_to(&mut s.clone(), 1, x, y)
}

/// A tile next to `slot` the leader can stand on (west, east, south, north first).
fn beside(s: &State, slot: Slot) -> Option<(u16, u16)> {
    beside_tile(s, s.objects.x(slot), s.objects.y(slot))
}

/// A tile next to (x, y) the leader can stand on (west, east, south, north first).
fn beside_tile(s: &State, x: u16, y: u16) -> Option<(u16, u16)> {
    let (x, y) = (i32::from(x), i32::from(y));
    [
        (-1, 0),
        (1, 0),
        (0, 1),
        (0, -1),
        (-1, 1),
        (1, 1),
        (-1, -1),
        (1, -1),
    ]
    .iter()
    .map(|(dx, dy)| (x + dx, y + dy))
    .find(|&(x, y)| walkable(s, x, y))
    .map(|(x, y)| (x as u16, y as u16))
}

/// True when `slot` lies outside the view around the centre ([`HALF_W`] columns and [`HALF_H`]
/// rows either side).
fn out_of_view(s: &State, slot: Slot) -> bool {
    let c = s.map.centre;
    let dx = i32::from(s.objects.x(slot)) - i32::from(c.x);
    let dy = i32::from(s.objects.y(slot)) - i32::from(c.y);
    dx.unsigned_abs() > HALF_W as u32 || dy.unsigned_abs() > HALF_H as u32
}

fn look_cmd() -> Command {
    Box::new(|st, io| Box::pin(async move { super::look(&st, &io).await }))
}

/// Look at an actor, a temporary actor (a hatched monster), a sign (0x8f: its text from
/// book.dat), a brazier (0xce), a weapon (0x71: its weight and damage) and a clock (0x9f: the
/// time), each the nearest of its type to the start; one out of view is looked at from a tile
/// beside it.
#[test]
fn look_at_six_things_near_the_start() {
    let Some(files) = files() else { return };
    let s0 = with_text(&files);
    let picture = find(&s0, 0x8f);
    let brazier = find(&s0, 0xce);
    let cleaver = find(&s0, 0x71);
    let clock = find(&s0, 0x9f);
    let mut all = Vec::new();
    for (what, slot) in [
        ("actor 5", 5),
        ("the first temporary actor", FIRST_TEMP_ACTOR),
        ("type 0x8f (a sign: book.dat)", picture),
        ("type 0xce", brazier),
        ("type 0x71 (weight, damage)", cleaver),
        ("type 0x9f (the time)", clock),
    ] {
        let mut s = s0.clone();
        if slot >= ACTOR_SLOTS as Slot && out_of_view(&s, slot) {
            let (x, y) = beside(&s, slot).unwrap_or_else(|| panic!("{what}: no tile beside it"));
            stand_at(&mut s, x, y);
        }
        aim(&mut s, slot);
        let (_, lines) = run(s, &files, &[], look_cmd());
        print_lines(what, &lines);
        all.push(lines.join("\n"));
    }
    // Shape only (the words are the user's game text): every Look starts with its lead-in,
    // the weapon line has a weight and a damage figure, the clock a time.
    let lead = super::text::string(&s0, 0x0012_8444);
    let lead = String::from_utf8_lossy(&lead).to_string();
    assert!(all.iter().all(|t| t.contains(lead.trim_end())));
    assert!(all[4].chars().filter(|c| c.is_ascii_digit()).count() >= 3);
    assert!(all[5].contains(':'));
}

/// A table near the start: the tile of a map object nearest the leader where Drop's surface
/// test holds (`surface_at`: an object whose tile has the surface bit, flags3 0x02, the branch
/// 0x12a226 takes for a table), with a tile beside it to stand on. Gives the table's tile and
/// that tile; found at run time (where tables stand is the user's world).
fn table(s: &State) -> Option<((u16, u16), (u16, u16))> {
    let c = s.map.centre;
    let mut tiles: Vec<(i32, u16, u16)> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&o| {
            let ob = &s.objects;
            ob.tf(o) != 0 && ob.place(o) == 0 && ob.z(o) == c.z
        })
        .map(|o| {
            let (x, y) = (s.objects.x(o), s.objects.y(o));
            let d = (i32::from(x) - i32::from(c.x)).abs() + (i32::from(y) - i32::from(c.y)).abs();
            (d, x, y)
        })
        .collect();
    tiles.sort_unstable();
    tiles.dedup();
    let mut probe = s.clone();
    tiles.into_iter().find_map(|(_, x, y)| {
        if !super::drop::surface_at(&mut probe, x, y) {
            return None;
        }
        Some(((x, y), beside_tile(s, x, y)?))
    })
}

fn carried_by(s: &State, owner: Slot, slot: Slot) -> bool {
    s.objects.place(slot) != 0 && s.objects.contained_in(slot, owner)
}

#[test]
fn get_the_bread_then_drop_it_on_a_table() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    // The nearest loaf (0x80) with a tile beside it to stand on.
    let (bread, (sx, sy)) = nearest(&s, 0x80, |s, o| beside(s, o).is_some())
        .into_iter()
        .find_map(|o| Some((o, beside(&s, o)?)))
        .expect("a loaf to stand by");
    stand_at(&mut s, sx, sy);
    aim(&mut s, bread);
    // Its weight: the type's byte of the weight table (tileflag 0x1000 + type, formats.md
    // §4.2), read from the raw file.
    let loaf_weight = i16::from(files.get("tileflag").quiet()[0x1000 + 0x80]);
    let weight = |s: &mut State| {
        super::seams::sum_inventory(s, 1);
        (s.inventory.equip_weight, s.inventory.carried_weight)
    };
    let before = weight(&mut s);
    let mp = s.actors.movement_points[1];
    let get = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
        Box::pin(async move { super::get(&st, &io).await }) as Pin<Box<dyn Future<Output = ()>>>
    });
    let (mut s, lines) = run(s, &files, &[], get);
    print_lines("Get the bread", &lines);
    let after = weight(&mut s);
    // The loaf stacks into the hero's inventory (0x10cf7c merges it into a stack he has or
    // keeps this object); either way the hero now carries a loaf and the weight rose by its
    // own; Get takes 3 movement points (0x1285b6's).
    let loaf = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .find(|&o| s.objects.kind(o) == 0x80 && carried_by(&s, 1, o))
        .expect("a loaf in the hero's inventory");
    assert!(
        after.1 - before.1 == loaf_weight,
        "the weight rose by the loaf's"
    );
    assert!(s.actors.movement_points[1] == mp.wrapping_sub(3));

    // Drop it on a table, the hero standing beside it (the inventory panel of member 0 shows
    // the loaf): the spot test's surface branch (`surface_at`) takes it.
    let ((ax, ay), (tx, ty)) = table(&s).expect("a table near the start");
    stand_at(&mut s, tx, ty);
    assert!(
        super::drop::surface_at(&mut s, ax, ay),
        "a surface to drop on"
    );
    s.ui.panel.mode = 0x92;
    s.ui.panel.member = 0;
    s.ui.input.selection = loaf;
    s.ui.input.target = (ax as i16, ay as i16);
    s.ui.input.targeting = 0;
    let drop = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
        Box::pin(async move { super::drop_object(&st, &io).await })
            as Pin<Box<dyn Future<Output = ()>>>
    });
    let (mut s, lines) = run(s, &files, &[InputEvent::Key(Key::Enter)], drop);
    print_lines("Drop it", &lines);
    let back = weight(&mut s);
    let on_floor = (0x100..crate::objects::OBJECT_SLOTS as Slot).find(|&o| {
        s.objects.kind(o) == 0x80
            && s.objects.place(o) == 0
            && (s.objects.x(o), s.objects.y(o)) == (ax, ay)
    });
    assert!(on_floor.is_some(), "the loaf lies on the table");
    assert!(back == before);
}

#[test]
fn push_a_chair_and_open_a_door() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    // The nearest chair (0xfc) with a free tile south of it and one east of it to stand on.
    let free = |s: &State, o: Slot, dx: i32, dy: i32| {
        walkable(
            s,
            i32::from(s.objects.x(o)) + dx,
            i32::from(s.objects.y(o)) + dy,
        )
    };
    let chair = *nearest(&s, 0xfc, |s, o| free(s, o, 0, 1) && free(s, o, 1, 0))
        .first()
        .expect("a chair with room to push it south");
    let (cx, cy) = (s.objects.x(chair), s.objects.y(chair));
    stand_at(&mut s, cx + 1, cy);
    aim(&mut s, chair);
    let push = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
        Box::pin(async move { super::push(&st, &io).await }) as Pin<Box<dyn Future<Output = ()>>>
    });
    // The direction prompt, then a direction: south.
    let (s, lines) = run(s, &files, &[InputEvent::Key(Key::Down)], push);
    print_lines("Move the chair south", &lines);
    assert!((s.objects.x(chair), s.objects.y(chair)) == (cx, cy + 1));

    // The nearest closed steel door (0x12c, frames 4..7 closed): open, then closed again.
    let mut s = with_text(&files);
    let door = *nearest(&s, 0x12c, |s, o| s.objects.tf(o) >> 10 & 4 != 0)
        .first()
        .expect("a closed door near the start");
    let closed = s.objects.tf(door) >> 10;
    let mut frames = Vec::new();
    for _ in 0..2 {
        super::door(&mut s, door, false, false, false);
        let open = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
            Box::pin(async move { crate::main_loop::flush_prints(&st, &io).await })
                as Pin<Box<dyn Future<Output = ()>>>
        });
        let (s2, lines) = run(s, &files, &[], open);
        s = s2;
        print_lines("the door used", &lines);
        frames.push(s.objects.tf(door) >> 10);
    }
    assert!(frames == [closed ^ 4, closed], "open, then closed");
}

/// Spec 08 Order 3 / Acceptance 5 (slices K2 and K3), for the object commands: the identity pack
/// of the owner's files (`crate::locale::identity`: every run, table string, catalogued
/// executable string, object name and book as itself, English grammar) installed (`wide` on),
/// the commands of the receipts above (Look at the six things, the sign's text from book.dat
/// among them, Get the bread and Drop it on a table, Move the chair, the door used twice) print
/// exactly the lines they print without it, and every catalogued string, object name and book
/// they show is one of the pack's (0 misses). Prints the counts of its own commands (runs, hits,
/// misses) and pass/fail only, no size of the pack.
#[test]
fn the_identity_pack_shows_look_get_drop_and_move_exactly_as_english() {
    let Some(files) = files() else { return };
    let pack = crate::locale::identity(&files).quiet();
    let english = with_text(&files);
    let mut with = english.clone();
    crate::locale::install(&mut with, &files, pack, None).quiet();
    assert!(with.ui.text.wide, "wide on");
    // Each command run on `s0`: its lines, and the counts after it (game strings, object names,
    // books: hits, misses).
    let commands = |s0: &State| -> Vec<(Vec<String>, [u32; 6])> {
        let mut out = Vec::new();
        let at = |s: &State| {
            let l = &s.locale;
            let ([a, b], [c, d], [e, f]) = (l.texts.get(), l.look.get(), l.books.get());
            [a, b, c, d, e, f]
        };
        for kind in [0x8f, 0xce, 0x71, 0x9f] {
            let mut s = s0.clone();
            let slot = find(&s, kind);
            if out_of_view(&s, slot) {
                let (x, y) = beside(&s, slot).quiet();
                stand_at(&mut s, x, y);
            }
            aim(&mut s, slot);
            let (s, lines) = run(s, &files, &[], look_cmd());
            out.push((lines, at(&s)));
        }
        for slot in [5, FIRST_TEMP_ACTOR] {
            let mut s = s0.clone();
            aim(&mut s, slot);
            let (s, lines) = run(s, &files, &[], look_cmd());
            out.push((lines, at(&s)));
        }
        let mut s = s0.clone();
        let (bread, (sx, sy)) = nearest(&s, 0x80, |s, o| beside(s, o).is_some())
            .into_iter()
            .find_map(|o| Some((o, beside(&s, o)?)))
            .quiet();
        stand_at(&mut s, sx, sy);
        aim(&mut s, bread);
        let get = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
            Box::pin(async move { super::get(&st, &io).await }) as Pin<Box<dyn Future<Output = ()>>>
        });
        let (mut s, lines) = run(s, &files, &[], get);
        out.push((lines, at(&s)));
        let ((ax, ay), (tx, ty)) = table(&s).quiet();
        stand_at(&mut s, tx, ty);
        let loaf = (0x100..crate::objects::OBJECT_SLOTS as Slot)
            .find(|&o| s.objects.kind(o) == 0x80 && carried_by(&s, 1, o))
            .quiet();
        s.ui.panel.mode = 0x92;
        s.ui.panel.member = 0;
        s.ui.input.selection = loaf;
        s.ui.input.target = (ax as i16, ay as i16);
        s.ui.input.targeting = 0;
        let drop = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
            Box::pin(async move { super::drop_object(&st, &io).await })
                as Pin<Box<dyn Future<Output = ()>>>
        });
        let (s, lines) = run(s, &files, &[InputEvent::Key(Key::Enter)], drop);
        out.push((lines, at(&s)));
        let mut s = s0.clone();
        let free = |s: &State, o: Slot, dx: i32, dy: i32| {
            walkable(
                s,
                i32::from(s.objects.x(o)) + dx,
                i32::from(s.objects.y(o)) + dy,
            )
        };
        let chair = *nearest(&s, 0xfc, |s, o| free(s, o, 0, 1) && free(s, o, 1, 0))
            .first()
            .quiet();
        let (cx, cy) = (s.objects.x(chair), s.objects.y(chair));
        stand_at(&mut s, cx + 1, cy);
        aim(&mut s, chair);
        let push = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
            Box::pin(async move { super::push(&st, &io).await })
                as Pin<Box<dyn Future<Output = ()>>>
        });
        let (s, lines) = run(s, &files, &[InputEvent::Key(Key::Down)], push);
        out.push((lines, at(&s)));
        let mut s = s0.clone();
        let door = *nearest(&s, 0x12c, |s, o| s.objects.tf(o) >> 10 & 4 != 0)
            .first()
            .quiet();
        for _ in 0..2 {
            super::door(&mut s, door, false, false, false);
            let open = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
                Box::pin(async move { crate::main_loop::flush_prints(&st, &io).await })
                    as Pin<Box<dyn Future<Output = ()>>>
            });
            let (s2, lines) = run(s, &files, &[], open);
            s = s2;
            out.push((lines, at(&s)));
        }
        out
    };
    let en = commands(&english);
    let id = commands(&with);
    let differ = en.iter().zip(&id).filter(|((a, _), (b, _))| a != b).count();
    let sum = |k: usize| -> u32 { id.iter().map(|(_, c)| c[k]).sum() };
    let (hits, misses) = (sum(0), sum(1));
    let (look_hits, look_misses, book_hits, book_misses) = (sum(2), sum(3), sum(4), sum(5));
    eprintln!(
        "identity pack, object commands: {} runs, differing {differ}; game strings from the pack {hits}, misses {misses}; object names shown {look_hits}, misses {look_misses}; books read {book_hits}, misses {book_misses}",
        id.len(),
    );
    assert_eq!(en.len(), id.len());
    assert_eq!(differ, 0, "every command's lines as in English");
    assert!(hits > 0, "the commands show the pack's strings");
    assert_eq!(misses, 0, "every catalogued string shown is in the pack");
    assert!(
        look_hits > 0 && book_hits > 0,
        "the pack's object names and the sign's book"
    );
    assert_eq!(
        (look_misses, book_misses),
        (0, 0),
        "every name and book shown is in the pack"
    );
    eprintln!("identity pack object commands identical to English: pass; misses 0: pass");
}

/// Spec 08 (slices K2 and K3): the string export of the owner's files (`u6.string-bundle/1`) lists
/// every catalogued string of his `game` once, each with its hash and length (it reads back),
/// none under three bytes but a label (one byte or more: a wind's name is two), of every kind;
/// Look's lead-in and the command names are among them; and the object names of his `look.lzd`
/// and the books of his `book.dat`, each once, as many as the identity pack holds. Prints
/// pass/fail only: no count or size of what his files hold (a size comparison fails with its
/// label, not the two sizes).
#[test]
fn the_string_export_of_the_owners_game_lists_its_catalogued_strings() {
    use crate::locale::strings::{self, Export, Kind};
    let Some(files) = files() else { return };
    let export = strings::export(&files).quiet();
    let back = Export::from_json(&export.to_json()).quiet();
    assert!(back == export, "it reads back");
    let s = with_text(&files);
    let has = |b: &[u8]| {
        let h = crate::converse::disasm::catalog::hash64(b);
        export.strings.iter().any(|e| e.hash == h)
    };
    let lead = super::text::string(&s, 0x0012_8444);
    let talk = s
        .objcmds
        .data
        .pointed(crate::image::A4.wrapping_sub(0x7c8e) + 4 * 0x83);
    let mut hashes: Vec<u64> = export.strings.iter().map(|e| e.hash).collect();
    hashes.sort_unstable();
    hashes.dedup();
    let count = |k: Kind| export.strings.iter().filter(|e| e.kind == k).count();
    let (messages, formats, labels) = (
        count(Kind::Message),
        count(Kind::Format),
        count(Kind::Label),
    );
    assert!(hashes.len() == export.strings.len(), "each once");
    assert!(
        export
            .strings
            .iter()
            .all(|e| e.len >= if e.kind == Kind::Label { 1 } else { 3 }),
        "none under three bytes but a label"
    );
    assert!(messages > 0 && formats > 0 && labels > 0);
    assert!(has(&lead) && has(&talk), "Look's lead-in and Talk's name");
    eprintln!("string export reads back, each string once, Look and Talk in it: pass");
    use crate::locale::texts::TextKind;
    let of = |k: TextKind| export.texts.iter().filter(|t| t.kind == k).count();
    let (look, books) = (of(TextKind::Look), of(TextKind::Book));
    let mut keys: Vec<(TextKind, u32)> = export.texts.iter().map(|t| (t.kind, t.index)).collect();
    keys.dedup();
    let pack = crate::locale::identity(&files).quiet();
    assert!(keys.len() == export.texts.len(), "each text once");
    assert!(
        (look, books) == (pack.look_entries(), pack.books()),
        "as many as the identity pack holds"
    );
    assert!(look > 0 && books > 0);
    eprintln!(
        "string export: object names and books each once, as many as the identity pack: pass"
    );
}

/// Spec 08 (slice K2, 2026-10-24): the string export of the owner's files holds combat's lines,
/// by the hashes of the strings at a hit's sites (`combat::damage`): the wound report's critical
/// line, its three degrees and its wounded word, the graze and the death line; and the two lines
/// that start with a newline, printed after Attack's held newline whole (the refusal of a
/// staff's cast, `combat::seams`; the next weapon's prompt, `combat::attack`). Prints pass/fail
/// only: a failure names the site, never its text.
#[test]
fn the_string_export_of_the_owners_game_holds_combats_wound_report_graze_and_death() {
    use crate::combat::{attack, damage, seams};
    use crate::image::{A4, GameImage};
    let Some(files) = files() else { return };
    let export = crate::locale::strings::export(&files).quiet();
    let img = GameImage::parse(files.get("game").quiet()).quiet();
    for (site, at, list) in [
        ("CRITICAL", A4 - 0x7750, damage::PRINTED),
        ("HEAVILY", A4 - 0x7745, damage::PRINTED),
        ("LIGHTLY", A4 - 0x773d, damage::PRINTED),
        ("BARELY", A4 - 0x7735, damage::PRINTED),
        ("WOUNDED", A4 - 0x772e, damage::PRINTED),
        ("GRAZED", A4 - 0x7723, damage::PRINTED),
        ("KILLED", A4 - 0x7716, damage::PRINTED),
        ("CAST_REFUSED", 0x0011_59f8, seams::PRINTED),
        ("NEXT_WEAPON", A4 - 0x76ac, attack::PRINTED),
    ] {
        assert!(list.contains(&at), "{site} is in its site's list");
        let h = crate::converse::disasm::catalog::hash64(&img.cstr(at).quiet());
        assert!(
            export.strings.iter().any(|e| e.hash == h),
            "{site} in the export"
        );
    }
    eprintln!(
        "string export holds combat's wound report, graze and death lines and its two newline-led lines: pass"
    );
}

/// Spec 08 (slice K2, 2026-10-24): the string export of the owner's files holds the inventory's
/// lines, by the hashes of the strings at Ready's and Unready's sites (`inventory::PRINTED`): the
/// Ready line, its three refusals and the Unready line; the locked chest's line
/// (`ui::click::PRINTED`); and the amulet's line as a label (`inventory::LABELS`, the pointer
/// A4-0x6c3c). The prompts of the command loop, the click and Ready are three strings at three
/// addresses, each of no word of its own: none is in the export. Prints pass/fail only: a
/// failure names the site, never its text.
#[test]
fn the_string_export_of_the_owners_game_holds_the_inventorys_ready_and_unready_lines() {
    use crate::converse::disasm::catalog::hash64;
    use crate::image::{A4, GameImage};
    use crate::locale::strings::Kind;
    let Some(files) = files() else { return };
    let export = crate::locale::strings::export(&files).quiet();
    let img = GameImage::parse(files.get("game").quiet()).quiet();
    let has = |text: &[u8]| export.strings.iter().any(|e| e.hash == hash64(text));
    for (site, at, list) in [
        ("READY", 0x0011_0c14, crate::inventory::PRINTED),
        ("CANT", 0x0011_0c24, crate::inventory::PRINTED),
        ("HEAVY", 0x0011_0c38, crate::inventory::PRINTED),
        ("NO_PLACE", 0x0011_0c46, crate::inventory::PRINTED),
        ("UNREADY", 0x0011_0c6a, crate::inventory::PRINTED),
        ("LOCKED", 0x0010_4fe6, crate::ui::click::PRINTED),
    ] {
        assert!(list.contains(&at), "{site} is in its site's list");
        assert!(has(&img.cstr(at).quiet()), "{site} in the export");
    }
    let amulet = A4 - 0x6c3c;
    assert!(
        crate::inventory::LABELS.contains(&amulet),
        "AMULET is in Unready's labels"
    );
    let h = hash64(&img.pointer(amulet).and_then(|p| img.cstr(p)).quiet());
    assert!(
        export
            .strings
            .iter()
            .any(|e| e.hash == h && e.kind == Kind::Label),
        "AMULET in the export as a label"
    );
    for (site, at, list) in [
        (
            "LOOP_PROMPT",
            0x0010_1f3c,
            crate::main_loop::commands::PRINTED,
        ),
        ("CLICK_PROMPT", 0x0010_4ff4, crate::ui::click::PRINTED),
        ("READY_PROMPT", 0x0011_0c5a, crate::inventory::PRINTED),
    ] {
        assert!(list.contains(&at), "{site} is in its site's list");
        assert!(!has(&img.cstr(at).quiet()), "{site} not in the export");
    }
    eprintln!(
        "string export holds the inventory's Ready, refusal, Unready, locked chest and amulet lines, and no prompt: pass"
    );
}

/// Spec 08 (slice K2, 2026-10-24): the string export of the owner's files holds magic's lines, by
/// the hashes of the strings at the spells' sites (`magic::PRINTED`): Cast's no-book line, the
/// books' no-magic, no-spells and nothing lines, the three refusals of the checks, the typed
/// spell's not-in-book line (A4-0x7387), the detections' lines and the not-possible line; the 13
/// result messages, the 8 potion descriptions and the 8 direction names as labels
/// (`magic::LABELS`, A4-0x6c6c + 4 n, A4-0x6b4c + 4 n and A4-0x61be + 4 n); (2026-10-25) the
/// spellbook page's strings, each with its cells: every spell's name that is not empty
/// (A4-0x6f4c + 4 n, `magic::NAME_LABELS`) and the 8 circles' names (A4-0x6c9c + 4 n,
/// `magic::CIRCLE_LABELS`) as labels within the page's fields, each English within it too, and
/// the level word (0x1158b4, `magic::PAGE_PRINTED`) within its English's cells; the 8 reagents'
/// names Look's description of a spell prints (A4-0x6d4c + 4 n, `objects::commands::LABELS`),
/// each not empty, as labels with no cells; and none of the incantations' syllables
/// (A4-0x70b8), which stay English, or the result formats of no word of their own. Prints
/// pass/fail only: a failure names the site, never its text.
#[test]
fn the_string_export_of_the_owners_game_holds_magics_lines_and_results_but_no_syllable() {
    use crate::converse::disasm::catalog::hash64;
    use crate::image::{A4, GameImage};
    use crate::locale::strings::Kind;
    let Some(files) = files() else { return };
    let export = crate::locale::strings::export(&files).quiet();
    let game = files.get("game").quiet();
    let img = GameImage::parse(game).quiet();
    let has = |text: &[u8]| export.strings.iter().any(|e| e.hash == hash64(text));
    let printed: Vec<u32> = crate::magic::PRINTED
        .iter()
        .flat_map(|l| l.iter().copied())
        .collect();
    for (site, at) in [
        ("NO_BOOK", 0x0011_5b36),
        ("NO_MAGIC", 0x0011_5a9a),
        ("NO_SPELLS", 0x0011_5ab2),
        ("NOTHING", 0x0011_5ad0),
        ("LEVEL_LOW", 0x0011_5ae8),
        ("NO_POINTS", 0x0011_5b0a),
        ("NO_REAGENTS", 0x0011_5b26),
        ("NOT_IN_BOOK", A4 - 0x7387),
        ("NOT_MAGICAL", 0x0011_5994),
        ("POTION", 0x0011_59a8),
        ("TRAPPED", 0x0011_595e),
        ("NO_TRAP", 0x0011_596e),
        ("NOT_POSSIBLE", 0x0011_59f8),
    ] {
        assert!(printed.contains(&at), "{site} is in magic's list");
        assert!(has(&img.cstr(at).quiet()), "{site} in the export");
    }
    // The result formats, the dispatcher's newline and the announcement's format hold no word
    // of their own: none is in the export.
    for (site, at) in [
        ("FMT_S", 0x0011_58b0),
        ("FMT_S_NL", 0x0011_58ca),
        ("FMT_NL_S_NL", 0x0011_58ce),
        ("FMT_NL_S_BANG", 0x0011_58e4),
        ("NEWLINE", 0x0011_58ba),
        ("FMT_NAME_QUOTE", 0x0011_5ae0),
    ] {
        assert!(printed.contains(&at), "{site} is in magic's list");
        assert!(!has(&img.cstr(at).quiet()), "{site} not in the export");
    }
    let label = |ptr: u32| {
        let h = hash64(&img.pointer(ptr).and_then(|p| img.cstr(p)).quiet());
        export
            .strings
            .iter()
            .any(|e| e.hash == h && e.kind == Kind::Label)
    };
    for (what, table, n) in [
        ("message", A4 - 0x6c6c, 13),
        ("potion", A4 - 0x6b4c, 8),
        ("direction", A4 - 0x61be, 8),
    ] {
        for i in 0..n {
            let ptr = table + 4 * i;
            assert!(
                crate::magic::LABELS.contains(&ptr),
                "{what} {i} is in magic's labels"
            );
            assert!(label(ptr), "{what} {i} in the export as a label");
        }
    }
    let tables = crate::magic::MagicTables::from_game(game).quiet();
    for (i, syllable) in tables.syllables.iter().enumerate() {
        assert!(!has(syllable), "syllable {i} not in the export");
    }
    // The spellbook page's strings, each within its field (2026-10-25).
    use crate::magic::{CIRCLE_CELLS, CIRCLE_LABELS, NAME_CELLS, NAME_LABELS, PAGE_PRINTED};
    let entry = |text: &[u8]| {
        export
            .strings
            .iter()
            .find(|e| e.hash == hash64(text))
            .map(|e| (e.kind, e.cells))
    };
    let cells_of = |text: &[u8]| u16::try_from(text.len()).ok();
    let mut named = 0;
    for (i, name) in tables.names.iter().enumerate() {
        let ptr = A4 - 0x6f4c + 4 * i as u32;
        assert!(NAME_LABELS.contains(&ptr), "spell {i} is in magic's names");
        if name.is_empty() {
            continue;
        }
        named += 1;
        assert!(
            cells_of(name).is_some_and(|c| c <= NAME_CELLS),
            "spell name {i}'s English within the page's field"
        );
        assert!(
            entry(name) == Some((Kind::Label, Some(NAME_CELLS))),
            "spell name {i} in the export as a label within the page's field"
        );
    }
    assert!(named > 0, "the spells have names");
    assert!(tables.circles.len() == 8, "eight circles");
    for (i, circle) in tables.circles.iter().enumerate() {
        let ptr = A4 - 0x6c9c + 4 * i as u32;
        assert!(
            CIRCLE_LABELS.contains(&ptr),
            "circle {i} is in magic's circles"
        );
        assert!(
            !circle.is_empty() && cells_of(circle).is_some_and(|c| c <= CIRCLE_CELLS),
            "circle name {i}'s English within the page's field"
        );
        assert!(
            entry(circle) == Some((Kind::Label, Some(CIRCLE_CELLS))),
            "circle name {i} in the export as a label within the page's field"
        );
    }
    let level = img.cstr(0x0011_58b4).quiet();
    assert!(
        !printed.contains(&0x0011_58b4) && PAGE_PRINTED == [0x0011_58b4],
        "the book page's level word is listed apart from magic's lines"
    );
    assert!(
        entry(&level).is_some_and(|(kind, cells)| kind != Kind::Label && cells == cells_of(&level)),
        "the book page's level word in the export within its English's cells"
    );
    // The reagents' names Look's description prints (2026-10-25), each on a wrapping line.
    for i in 0..8u32 {
        let ptr = A4 - 0x6d4c + 4 * i;
        assert!(
            crate::objects::commands::LABELS.contains(&ptr),
            "reagent {i} is in the object commands' labels"
        );
        let name = img.pointer(ptr).and_then(|p| img.cstr(p)).quiet();
        assert!(!name.is_empty(), "reagent {i} has a name");
        assert!(
            entry(&name) == Some((Kind::Label, None)),
            "reagent name {i} in the export as a label with no cells"
        );
    }
    eprintln!(
        "string export holds magic's refusals, detections and not-in-book line, its 13 result messages, 8 potions and 8 directions as labels, the spells' and circles' names as labels within the book page's fields and its level word within its cells, the 8 reagents' names as labels, and no result format or syllable: pass"
    );
}

/// Spec 08 (slice K2, 2026-10-24): the string export of the owner's files holds the panels'
/// lines, by the hashes of the strings at the panel sites, each with its English's cells (its
/// length): the stats view's three attribute lines and its magic, health and level headings
/// (`ui::tables::PRINTED`), and the inventory view's two weight labels as labels, read for
/// display at their addresses (`ui::tables::SHOWN_AT`; under three bytes, so no print finds
/// them); the seven combat-mode names as labels (`ui::tables::LABELS`, A4-0x7230 + 4 × mode),
/// each within the line's 7 cells; none of the panel formats it leaves out (the roster's number,
/// the name line, the bare string, the values); and `cells` on no other entry but the
/// spellbook page's (2026-10-25: its spells' and circles' names and its level word, the test
/// above). Prints pass/fail only: a failure names the site, never its text.
#[test]
fn the_string_export_of_the_owners_game_holds_the_panels_lines_and_mode_names_with_their_cells() {
    use crate::converse::disasm::catalog::hash64;
    use crate::image::{A4, GameImage};
    use crate::locale::strings::Kind;
    use crate::ui::tables::{LABELS, MODE_CELLS, PRINTED, SHOWN_AT};
    let Some(files) = files() else { return };
    let export = crate::locale::strings::export(&files).quiet();
    let img = GameImage::parse(files.get("game").quiet()).quiet();
    let find = |text: &[u8]| export.strings.iter().find(|e| e.hash == hash64(text));
    let cells_of = |text: &[u8]| u16::try_from(text.len()).ok();
    let mut budgeted = Vec::new();
    for (site, at) in [
        ("STRENGTH", 0x0011_0bb2),
        ("DEXTERITY", 0x0011_0bba),
        ("INTELLIGENCE", 0x0011_0bc2),
        ("MAGIC", 0x0011_0bca),
        ("HEALTH", 0x0011_0bd8),
        ("LEVEL_EXP", 0x0011_0bea),
    ] {
        assert!(PRINTED.contains(&at), "{site} is in the panels' list");
        let text = img.cstr(at).quiet();
        let e = find(&text);
        assert!(e.is_some(), "{site} in the export");
        assert!(
            e.is_some_and(|e| e.cells == cells_of(&text)),
            "{site} with its English's cells"
        );
        budgeted.push(hash64(&text));
    }
    for (site, at) in [("WEIGHT_0", 0x0011_0c0c), ("WEIGHT_1", 0x0011_0c10)] {
        assert!(
            SHOWN_AT.contains(&at) && !PRINTED.contains(&at),
            "{site} is in the panels' labels read at their addresses"
        );
        let text = img.cstr(at).quiet();
        let e = find(&text);
        assert!(
            e.is_some_and(|e| e.kind == Kind::Label),
            "{site} in the export as a label"
        );
        assert!(
            e.is_some_and(|e| e.cells == cells_of(&text)),
            "{site} with its English's cells"
        );
        budgeted.push(hash64(&text));
    }
    for mode in 2..=8u32 {
        let ptr = A4 - 0x7230 + 4 * mode;
        assert!(
            LABELS.contains(&ptr),
            "mode {mode} is in the panels' labels"
        );
        let text = img.pointer(ptr).and_then(|p| img.cstr(p)).quiet();
        let e = find(&text);
        assert!(
            e.is_some_and(|e| e.kind == Kind::Label),
            "mode {mode} in the export as a label"
        );
        assert!(
            e.is_some_and(|e| e.cells == Some(MODE_CELLS)),
            "mode {mode} within the line's cells"
        );
        budgeted.push(hash64(&text));
    }
    for (site, at) in [
        ("ROSTER_HP", 0x0011_0ba4),
        ("NAME_LINE", 0x0011_0baa),
        ("PLAIN", 0x0011_0bae),
        ("MAGIC_VALUE", 0x0011_0bd0),
        ("HP_VALUE", 0x0011_0be0),
        ("HP_MAX", 0x0011_0be4),
        ("LEVEL_EXP_VALUE", 0x0011_0bf4),
    ] {
        assert!(!PRINTED.contains(&at), "{site} is not in the panels' list");
        assert!(
            find(&img.cstr(at).quiet()).is_none(),
            "{site} not in the export"
        );
    }
    // The spellbook page's strings carry cells too (2026-10-25).
    let book = crate::magic::NAME_LABELS
        .iter()
        .chain(&crate::magic::CIRCLE_LABELS)
        .filter_map(|&ptr| img.pointer(ptr).and_then(|p| img.cstr(p)).ok())
        .chain(
            crate::magic::PAGE_PRINTED
                .iter()
                .map(|&at| img.cstr(at).quiet()),
        );
    budgeted.extend(book.map(|text| hash64(&text)));
    assert!(
        export
            .strings
            .iter()
            .all(|e| e.cells.is_none() || budgeted.contains(&e.hash)),
        "no other entry carries cells"
    );
    eprintln!(
        "string export holds the panels' attribute lines and headings with their cells, the 2 weight labels and the 7 combat-mode names as labels within their cells, none of the panel formats left out, and cells on no other entry: pass"
    );
}

/// The keys (type 0x40) of quality `q` the port holds, anywhere (the slot list).
fn keys_of(s: &State, q: u8) -> Vec<Slot> {
    let o = &s.objects;
    let mut v = Vec::new();
    let mut c = o.next_of(crate::objects::HEAD);
    let mut n = 0;
    while c >= 0 && n < crate::objects::OBJECT_SLOTS {
        if o.kind(c) == 0x40 && o.qq_of(c) >> 8 == u16::from(q) {
            v.push(c);
        }
        c = o.next_of(c);
        n += 1;
    }
    v
}

/// Points the cross-hair at the map tile (x, y) with no object selected.
fn aim_tile(s: &mut State, x: u16, y: u16) {
    let pointer = cell_pixel(s, x, y);
    let input = &mut s.ui.input;
    input.selection = -1;
    input.target = (x as i16, y as i16);
    input.pointer = pointer;
    input.target_void = false;
}

/// Look with the screen drawn before and after it (the paper, the view, the panel), as the
/// command loop leaves it, so the frame after it shows the map.
fn look_drawn() -> Command {
    Box::new(|st, io| {
        Box::pin(async move {
            crate::main_loop::draw_paper(&mut st.borrow_mut(), &io);
            crate::gfx::refresh_view(&mut st.borrow_mut(), &io);
            crate::ui::panels::redraw_panel(&st, &io).await;
            super::look(&st, &io).await;
            crate::main_loop::flush_prints(&st, &io).await;
            crate::gfx::refresh_view(&mut st.borrow_mut(), &io);
            io.present();
        })
    })
}

/// Port-only (owner, 2026-10-21; `super::gatehouse`): in a new game every gatehouse plant the
/// rule finds, looked at from a tile beside it, gives one key of its door's lock (the port holds
/// none before) on its tile; the hero takes it, and Use of it on that door unlocks the door
/// (0x12bbc0's locked branch). Prints each door's index with the number of plants whose key is
/// its, each plant's index and pass/fail only: where the plants and doors are, their tiles and
/// the lock are the user's world. With `U6_GK_FRAMES=<dir>` it writes the frame after each
/// plant's Look there (`gk-<plant index>.png`).
#[test]
fn every_gatehouse_plant_gives_the_key_that_unlocks_its_door() {
    let Some(files) = files() else { return };
    let mut s0 = with_text(&files);
    s0.gfx.window_shown = 1;
    s0.ui.panel.dirty = true;
    let z = usize::from(s0.map.centre.z);
    let doors = super::gatehouse::doors(&s0);
    assert!(!doors.is_empty(), "the new game has gatehouse doors");
    let reach = super::gatehouse::PLANT_REACH;
    let mut plants: Vec<(u16, u16, Slot)> = Vec::new();
    for &d in &doors {
        let (dx, dy) = (i32::from(s0.objects.x(d)), i32::from(s0.objects.y(d)));
        for y in dy - reach..=dy + reach {
            for x in dx - reach..=dx + reach {
                let (x, y) = (x as u16, y as u16);
                let Some(tile) = s0.map.tile_at(usize::from(x), usize::from(y), z) else {
                    continue;
                };
                if let Some(door) = super::gatehouse::plant_door(&s0, x, y, tile) {
                    plants.push((x, y, door));
                }
            }
        }
    }
    plants.sort_unstable();
    plants.dedup();
    for (k, &d) in doors.iter().enumerate() {
        let n = plants.iter().filter(|p| p.2 == d).count();
        eprintln!("door {k}: plants that give its key {n}");
    }
    assert!(!plants.is_empty(), "the rule finds gatehouse plants");
    for (i, &(px, py, door)) in plants.iter().enumerate() {
        let mut s = s0.clone();
        let q = (s.objects.qq_of(door) >> 8) as u8;
        assert!(
            keys_of(&s, q).is_empty(),
            "plant {i}: no key of the lock before"
        );
        let (sx, sy) =
            beside_tile(&s, px, py).unwrap_or_else(|| panic!("plant {i}: no tile beside it"));
        stand_at(&mut s, sx, sy);
        aim_tile(&mut s, px, py);
        let (e, lines) = run_engine(s, &files, &[], look_drawn());
        print_lines(&format!("plant {i}: Look"), &lines);
        crate::usecmd::tests_real::save_frame(&e, "U6_GK_FRAMES", &format!("gk-{i}"));
        let mut s = e.state().clone();
        let made = keys_of(&s, q);
        assert!(made.len() == 1, "plant {i}: one key of the lock");
        let key = made[0];
        let o = &s.objects;
        assert!(
            o.place(key) == 0 && (o.x(key), o.y(key)) == (px, py),
            "plant {i}: the key lies on the plant's tile"
        );

        // The hero takes it.
        aim(&mut s, key);
        let get = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
            Box::pin(async move { super::get(&st, &io).await }) as Pin<Box<dyn Future<Output = ()>>>
        });
        let (mut s, lines) = run(s, &files, &[], get);
        print_lines(&format!("plant {i}: Get the key"), &lines);
        assert!(
            carried_by(&s, 1, key),
            "plant {i}: the hero carries the key"
        );

        // Use on its door from a tile beside it; the target is confirmed as the repeat key gives
        // it (0x10790a returns 0x8e).
        let (bx, by) = beside(&s, door).unwrap_or_else(|| panic!("plant {i}: no tile by the door"));
        stand_at(&mut s, bx, by);
        let locked = s.objects.tf(door) >> 10;
        assert!((8..=11).contains(&locked), "plant {i}: the door is locked");
        aim(&mut s, door);
        s.ui.input.targeting = 0;
        s.ui.input.repeat = true;
        let use_key = Box::new(move |st: Rc<RefCell<State>>, io: Rc<Io>| {
            Box::pin(async move { super::use_key(&st, &io, key).await })
                as Pin<Box<dyn Future<Output = ()>>>
        });
        let (s, lines) = run(s, &files, &[], use_key);
        print_lines(&format!("plant {i}: Use the key on the door"), &lines);
        assert!(
            s.objects.tf(door) >> 10 == locked ^ 0xc,
            "plant {i}: the door unlocked"
        );
        eprintln!("plant {i}: key given, taken, door unlocked: pass");
    }
}

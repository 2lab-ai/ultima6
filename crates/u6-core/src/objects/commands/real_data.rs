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
/// they show is one of the pack's (0 misses), and (2026-10-27, spec 16 item 1) every executable
/// string they show is catalogued (0 uncatalogued). Prints the counts of its own commands (runs,
/// hits, misses, uncatalogued) and pass/fail only, no size of the pack.
#[test]
fn the_identity_pack_shows_look_get_drop_and_move_exactly_as_english() {
    let Some(files) = files() else { return };
    let pack = crate::locale::identity(&files).quiet();
    let english = with_text(&files);
    let mut with = english.clone();
    crate::locale::install(&mut with, &files, pack, None).quiet();
    assert!(with.ui.text.wide, "wide on");
    // Each command run on `s0`: its lines, and the counts after it (game strings, object names,
    // books: hits, misses; game strings uncatalogued).
    let commands = |s0: &State| -> Vec<(Vec<String>, [u32; 7])> {
        let mut out = Vec::new();
        let at = |s: &State| {
            let l = &s.locale;
            let ([a, b], [c, d], [e, f]) = (l.texts.get(), l.look.get(), l.books.get());
            [a, b, c, d, e, f, l.texts.uncatalogued.get()]
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
    let uncatalogued = sum(6);
    eprintln!(
        "identity pack, object commands: {} runs, differing {differ}; game strings from the pack {hits}, misses {misses}, uncatalogued {uncatalogued}; object names shown {look_hits}, misses {look_misses}; books read {book_hits}, misses {book_misses}",
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
    assert_eq!(
        uncatalogued, 0,
        "every executable string shown is catalogued"
    );
    eprintln!(
        "identity pack object commands identical to English: pass; misses 0: pass; uncatalogued 0: pass"
    );
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
/// line, its three degrees and its wounded word, the graze and the death line; the next weapon's
/// prompt, printed after Attack's held newline whole, and the charm and paralysis reports after
/// a spellbook's or spell object's cast (`combat::attack`). Prints pass/fail only: a failure
/// names the site, never its text.
#[test]
fn the_string_export_of_the_owners_game_holds_combats_wound_report_graze_and_death() {
    use crate::combat::{attack, damage};
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
        ("NEXT_WEAPON", A4 - 0x76ac, attack::PRINTED),
        ("CHARMED", 0x0012_1ba0, attack::PRINTED),
        ("PARALYZED", 0x0012_1bb0, attack::PRINTED),
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
/// above) and (2026-10-27, spec 16 item 1) the inventory view's weight values, with the
/// English's cells (`inventory::PANEL_PRINTED`). Prints pass/fail only: a failure names
/// the site, never its text.
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
    let weights = img.cstr(0x0011_0c04).quiet();
    assert!(
        find(&weights).is_some_and(|e| e.cells == cells_of(&weights)),
        "the weight values with their English's cells"
    );
    budgeted.push(hash64(&weights));
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

/// Spec 08 (slice K2, 2026-10-25): the string export of the owner's files holds Use's own lines,
/// by the hashes of the strings at its sites (`usecmd::PRINTED`, each file's list): the lever's
/// line and its three outcomes (0x12cad2, 0x12cae6, 0x12cb02, 0x12cb16), the drawbridge crank's
/// no-bridge line and its five results (0x12ca2a, 0x12ca48 .. 0x12cab4), and every other site's
/// string with a word of its own, with no cell budget (each on a wrapping line); and none of the
/// ten of no word of their own (the three line formats, the newline, the direction's format,
/// the wand's two syllable formats, the well's message format and the mark that ends its food
/// line, the astrolabe's out-of-range line). Prints pass/fail and counts only: the export's
/// strings, each list's sites and how many of them are in it; a failure names the site, never
/// its text.
#[test]
fn the_string_export_of_the_owners_game_holds_uses_own_lines() {
    use crate::converse::disasm::catalog::hash64;
    use crate::image::GameImage;
    let Some(files) = files() else { return };
    let export = crate::locale::strings::export(&files).quiet();
    let img = GameImage::parse(files.get("game").quiet()).quiet();
    let entry = |at: u32| {
        let h = hash64(&img.cstr(at).quiet());
        export.strings.iter().find(|e| e.hash == h)
    };
    let no_word: [u32; 10] = [
        0x0012_c9ee,
        0x0012_c93e,
        0x0012_cda6,
        0x0012_ccfe,
        0x0012_cd16,
        0x0012_c94a,
        0x0012_c94e,
        0x0012_ccf8,
        0x0012_ccf4,
        0x0012_cec2,
    ];
    let names = ["the checks'", "food's", "mech's", "misc's", "travel's"];
    for (name, list) in names.into_iter().zip(crate::usecmd::PRINTED) {
        let mut held = 0;
        for &at in list {
            let e = entry(at);
            if no_word.contains(&at) {
                assert!(
                    e.is_none(),
                    "{name} site {at:#x}, of no word, not in the export"
                );
            } else {
                assert!(e.is_some(), "{name} site {at:#x} in the export");
                assert!(
                    e.is_some_and(|e| e.cells.is_none()),
                    "{name} site {at:#x} with no cell budget"
                );
                held += 1;
            }
        }
        eprintln!(
            "Use's {name} list: {} sites, {held} in the export: pass",
            list.len()
        );
    }
    let mech = crate::usecmd::PRINTED[2];
    for (site, at) in [
        ("LEVER", 0x0012_cad2),
        ("STRANGE", 0x0012_cae6),
        ("NOISE", 0x0012_cb02),
        ("CANT_CLOSE", 0x0012_cb16),
        ("NO_BRIDGE", 0x0012_ca2a),
        ("BRIDGE_0", 0x0012_ca48),
        ("BRIDGE_1", 0x0012_ca66),
        ("BRIDGE_2", 0x0012_ca7e),
        ("BRIDGE_3", 0x0012_ca96),
        ("BRIDGE_4", 0x0012_cab4),
    ] {
        assert!(mech.contains(&at), "{site} is in mech's list");
        assert!(entry(at).is_some(), "{site} in the export");
    }
    eprintln!("string export: {} strings", export.strings.len());
    eprintln!(
        "string export holds the lever's and the drawbridge crank's lines and every Use line with a word of its own, none of the ten of no word, no cell budget on any: pass"
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

fn push_cmd() -> Command {
    Box::new(|st, io| Box::pin(async move { super::push(&st, &io).await }))
}

/// The first object the hero carries in his inventory (not readied) that is neither a stack
/// nor weightless nor a container: found at run time.
fn plain_carried(s: &State, hero: Slot) -> Slot {
    (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .find(|&o| {
            let ob = &s.objects;
            let k = ob.kind(o);
            ob.place(o) == crate::objects::IN_INVENTORY
                && ob.parent(o) == hero
                && k != 0x4c
                && crate::objects::stack_kind(ob, k) == 0
                && !crate::objects::in_type_set_670c(ob, k)
                && super::seams::weight(s, o) > 0
        })
        .expect("a plain object the hero carries")
}

/// A container type of the 9 at A4-0x670c (0x12899c) that none of Move's container rules
/// singles out (not the spellbook 0x39, the vortex cube 0x3e, a chest or 0x153).
fn bag_type(s: &State) -> u16 {
    *s.objects.defs.type_set_670c[..9]
        .iter()
        .find(|&&k| ![0x39, 0x3e, 0x62, 0xba, 0xc0, 0x153].contains(&k))
        .expect("a container type")
}

/// Move (0x12ae04) on carried `obj` as a pick from the inventory panel leaves it: the panel
/// shows `shown` (mode 0x92, member 0, its list cells as its draw 0x1118c6 fills them), the
/// panel cursor is up and the command loop waits for the target (A4-0x7a32 = 1, command 0x87). `target`: the panel cell it is in (the figure for
/// `shown` itself), for Tab and Return to pick; else `keys` pick it.
fn move_carried(
    mut s: State,
    files: &Files,
    obj: Slot,
    shown: Slot,
    target: Option<Slot>,
    keys: &[InputEvent],
) -> (Engine, Vec<String>) {
    s.ui.panel.mode = 0x92;
    s.ui.panel.member = 0;
    s.ui.panel.container = shown as u16;
    crate::inventory::recount(&mut s, shown);
    let listed = listed_children(&mut s, shown);
    for (k, cell) in s.inventory.cells[..12].iter_mut().enumerate() {
        *cell = listed.get(k).copied().unwrap_or(0);
    }
    let cursor = match target {
        Some(t) if t == shown => (3, -1),
        Some(t) => {
            let k = listed[..listed.len().min(12)]
                .iter()
                .position(|&c| c == t)
                .expect("the target on the shown list's first page") as i8;
            (3 + k % 4, k / 4)
        }
        None => (0, 0),
    };
    let input = &mut s.ui.input;
    input.selection = obj;
    input.target_pending = 1;
    input.target_command = 0x87;
    input.targeting = 2;
    input.panel_cursor = cursor;
    run_engine(s, files, keys, push_cmd())
}

/// The entries the inventory list of `shown` holds, in its order (0x1118c6: the children in
/// the inventory or a container, not readied).
fn listed_children(s: &mut State, shown: Slot) -> Vec<Slot> {
    let mut v = Vec::new();
    let mut c = crate::objects::first_child(&mut s.objects, shown);
    while c >= 0 {
        let p = s.objects.place(c);
        if p == crate::objects::IN_INVENTORY || p == crate::objects::IN_CONTAINER {
            v.push(c);
        }
        c = crate::objects::next_child(&mut s.objects);
    }
    v
}

const TAB_RETURN: [InputEvent; 2] = [
    InputEvent::Key(Key::Char('\t')),
    InputEvent::Key(Key::Enter),
];

/// The words of the game string at `addr`, in order, among the words of `lines` (the scroll
/// wraps a line where it likes).
fn printed(s: &State, lines: &[String], addr: u32) -> bool {
    let words = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    let want = words(&String::from_utf8_lossy(&super::text::string(s, addr)));
    !want.is_empty() && words(&lines.join(" ")).contains(&want)
}

/// The end of a Move that moved (0x12ba24: no target pending, the panel not marked; port-only in
/// Wide: marked for the three views' redraw, spec 14 Design 2).
fn ended(s: &State) -> bool {
    s.ui.input.target_pending == 0 && s.ui.panel.dirty == cfg!(feature = "wide")
}

/// The leader's movement points.
fn leader_mp(s: &State) -> u8 {
    let p = &s.actors.party;
    s.actors.movement_points[usize::from(p.members[usize::from(p.leader_index)])]
}

/// Move on a carried object, the target another member (0x12b38a..0x12b5ba): the second
/// member's digit picks him; he is weighed (0x11121e, his readied and carried weight plus the
/// object's with its contents, 0x1110f6 + 0x1111ca) against his strength × 20 (0x12b482), the
/// object goes into his inventory (0x10cda8 with place 0x10, 0x12b594) and the leader spends 5
/// movement points (0x12b60a).
#[test]
fn move_gives_a_carried_object_to_another_member() {
    let Some(files) = files() else { return };
    let s0 = with_text(&files);
    let hero = Slot::from(s0.actors.party.members[0]);
    let other = Slot::from(s0.actors.party.members[1]);
    let obj = plain_carried(&s0, hero);
    let mut probe = s0.clone();
    let w = super::seams::weight(&probe, obj) + super::seams::contents_weight(&probe, obj);
    super::seams::sum_inventory(&mut probe, other);
    let before = probe.inventory.carried_weight;
    let keys = [InputEvent::Key(Key::Char('2'))];
    let (e, lines) = move_carried(s0.clone(), &files, obj, hero, None, &keys);
    print_lines("Move to the second member", &lines);
    let mut s = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(s.objects.parent(obj) == other, "the second member holds it");
    assert!(s.objects.place(obj) == crate::objects::IN_INVENTORY);
    super::seams::sum_inventory(&mut s, other);
    assert!(
        s.inventory.carried_weight == before + w,
        "his weight rose by it"
    );
    assert!(leader_mp(&s) == leader_mp(&s0).wrapping_sub(5));
    assert!(ended(&s), "the end of a Move (0x12ba24)");
}

/// The same Move to a member whose strength × 20 is below what he would carry: the can't-carry
/// line (0x128662, 0x12b48a), the shown member's sums redone (0x12b4a2), nothing moves and no
/// movement points are spent (the return at 0x12b4a6 comes before 0x12b60a).
#[test]
fn move_refuses_a_member_who_cannot_carry_it() {
    let Some(files) = files() else { return };
    let mut s0 = with_text(&files);
    let hero = Slot::from(s0.actors.party.members[0]);
    let other = Slot::from(s0.actors.party.members[1]);
    s0.actors.attrs.strength[other as usize] = 0;
    let obj = plain_carried(&s0, hero);
    let keys = [InputEvent::Key(Key::Char('2'))];
    let (e, lines) = move_carried(s0.clone(), &files, obj, hero, None, &keys);
    print_lines("Move to a member too weak", &lines);
    let s = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(printed(&s0, &lines, 0x0012_8662), "the can't-carry line");
    assert!(s.objects.parent(obj) == hero, "still the hero's");
    assert!(s.objects.place(obj) == crate::objects::IN_INVENTORY);
    assert!(leader_mp(&s) == leader_mp(&s0));
    assert!(s.ui.input.target_pending == 1, "still pending (0x12ba2c)");
}

/// Move on a carried object, the target a container the same member carries, picked on the
/// panel (Tab, Return): into it (the into line 0x1287d0, 0x10cda8 with place 0x08, 0x12b9e0);
/// then, the panel showing that container, the object moved onto the container's figure goes
/// out of it (the out-of line 0x1287c8) to the container's holder with place 0x08 too
/// (0x12b8ba..0x12b8e6). Each spends the leader 5 movement points (0x12b914, 0x12b7d0).
#[test]
fn move_puts_a_carried_object_into_a_bag_and_takes_it_out() {
    let Some(files) = files() else { return };
    let mut s0 = with_text(&files);
    let hero = Slot::from(s0.actors.party.members[0]);
    let obj = plain_carried(&s0, hero);
    let bag = bag_type(&s0);
    let bag = crate::objects::create_in(&mut s0, bag, crate::objects::IN_INVENTORY, 0, hero);
    assert!(bag >= 0, "a bag for the hero");
    let (e, lines) = move_carried(s0.clone(), &files, obj, hero, Some(bag), &TAB_RETURN);
    print_lines("Move into the bag", &lines);
    let s1 = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(printed(&s0, &lines, 0x0012_87d0), "the into line");
    assert!(s1.objects.parent(obj) == bag, "in the bag");
    assert!(s1.objects.place(obj) == crate::objects::IN_CONTAINER);
    assert!(leader_mp(&s1) == leader_mp(&s0).wrapping_sub(5));
    assert!(ended(&s1), "the end of a Move (0x12ba24)");

    let (e, lines) = move_carried(s1.clone(), &files, obj, bag, Some(bag), &TAB_RETURN);
    print_lines("Move out of the bag", &lines);
    let s2 = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(printed(&s0, &lines, 0x0012_87c8), "the out-of line");
    assert!(s2.objects.parent(obj) == hero, "back with the hero");
    assert!(s2.objects.place(obj) == crate::objects::IN_CONTAINER);
    assert!(leader_mp(&s2) == leader_mp(&s1).wrapping_sub(5));
    assert!(ended(&s2), "the end of a Move (0x12ba24)");
}

/// Move on a carried object that is not a spell (0x3a) onto a spellbook (0x39) the same member
/// carries: the only-spells line (0x128730, 0x12b6ca..0x12b706); nothing moves and no movement
/// points are spent. The rule is the target's: a spellbook takes only spells.
#[test]
fn move_puts_only_a_spell_into_a_spellbook() {
    let Some(files) = files() else { return };
    let mut s0 = with_text(&files);
    let hero = Slot::from(s0.actors.party.members[0]);
    let obj = plain_carried(&s0, hero);
    assert!(s0.objects.kind(obj) != 0x3a);
    assert!(crate::objects::in_type_set_670c(&s0.objects, 0x39));
    let book = crate::objects::create_in(&mut s0, 0x39, crate::objects::IN_INVENTORY, 0, hero);
    assert!(book >= 0, "a spellbook for the hero");
    let (e, lines) = move_carried(s0.clone(), &files, obj, hero, Some(book), &TAB_RETURN);
    print_lines("Move into the spellbook", &lines);
    let s = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(printed(&s0, &lines, 0x0012_8730), "the only-spells line");
    assert!(s.objects.parent(obj) == hero, "still the hero's");
    assert!(leader_mp(&s) == leader_mp(&s0));
    assert!(s.ui.input.target_pending == 1, "still pending (0x12ba2c)");
}

/// The readied and carried weight sums 0x11121e leaves for actor `a`.
fn sums_of(s: &State, a: Slot) -> (i16, i16) {
    let mut p = s.clone();
    super::seams::sum_inventory(&mut p, a);
    (p.inventory.equip_weight, p.inventory.carried_weight)
}

/// Move to a member who cannot carry it, the panel paged on twice at the to prompt (`+`: 0x99,
/// read inside the target key 0x10790a): the sums the refusal redoes are those of the member
/// the panel shows then (0x12b492 reads A4-0x7a3e again), not the giver's (kept at 0x12b2be
/// for the yourself test, 0x11236c and the readied slots). The target stays pending (the
/// return at 0x12b4a6 skips 0x12ba24).
#[test]
fn move_refused_redoes_the_sums_of_the_member_the_panel_shows() {
    let Some(files) = files() else { return };
    let mut s0 = with_text(&files);
    let p = s0.actors.party.members;
    let (hero, other, third) = (Slot::from(p[0]), Slot::from(p[1]), Slot::from(p[2]));
    s0.actors.attrs.strength[other as usize] = 0;
    let obj = plain_carried(&s0, hero);
    let third_sums = sums_of(&s0, third);
    assert!(sums_of(&s0, hero) != third_sums, "the giver's sums differ");
    assert!(
        sums_of(&s0, other) != third_sums,
        "the target's sums differ"
    );
    let keys = [
        InputEvent::Key(Key::Char('+')),
        InputEvent::Key(Key::Char('+')),
        InputEvent::Key(Key::Char('2')),
    ];
    let (e, lines) = move_carried(s0.clone(), &files, obj, hero, None, &keys);
    print_lines("Move after paging the panel", &lines);
    let s = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(printed(&s0, &lines, 0x0012_8662), "the can't-carry line");
    assert!(
        s.ui.panel.member == 2,
        "the panel paged to the third member"
    );
    let now = (s.inventory.equip_weight, s.inventory.carried_weight);
    assert!(now == third_sums, "the third member's sums redone");
    assert!(s.objects.parent(obj) == hero, "still the hero's");
    assert!(s.ui.input.target_pending == 1, "still pending");
}

/// Move to a member who takes it, the panel paged on twice at the to prompt, the panel's view
/// the stats (0x90: the end of Move redraws no inventory, 0x12ba14): the sums the move leaves
/// (0x12b5a6 reads A4-0x7a3e again) are those of the member the panel shows then, not the
/// giver's nor the recipient's. Read from the state the command left, before any recount here.
#[test]
fn move_given_redoes_the_sums_of_the_member_the_panel_shows() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let p = s.actors.party.members;
    let (hero, other, third) = (Slot::from(p[0]), Slot::from(p[1]), Slot::from(p[2]));
    let obj = plain_carried(&s, hero);
    let third_sums = sums_of(&s, third);
    assert!(sums_of(&s, hero) != third_sums, "the giver's sums differ");
    assert!(
        sums_of(&s, other) != third_sums,
        "the recipient's sums differ"
    );
    s.ui.panel.mode = 0x90;
    s.ui.panel.member = 0;
    s.ui.panel.container = hero as u16;
    let input = &mut s.ui.input;
    input.selection = obj;
    input.target_pending = 1;
    input.target_command = 0x87;
    input.targeting = 2;
    let keys = [
        InputEvent::Key(Key::Char('+')),
        InputEvent::Key(Key::Char('+')),
        InputEvent::Key(Key::Char('2')),
    ];
    let (e, lines) = run_engine(s, &files, &keys, push_cmd());
    print_lines("Move after paging the panel, taken", &lines);
    let s = e.state().clone();
    let left = (s.inventory.equip_weight, s.inventory.carried_weight);
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(
        s.ui.panel.member == 2,
        "the panel paged to the third member"
    );
    assert!(s.objects.parent(obj) == other, "the second member holds it");
    assert!(
        sums_of(&s, hero) != third_sums,
        "the giver's sums still differ"
    );
    assert!(
        sums_of(&s, other) != third_sums,
        "the recipient's sums still differ"
    );
    assert!(left == third_sums, "the third member's sums redone");
    assert!(ended(&s), "the end of a Move (0x12ba24)");
}

/// Spec 14 (Wide): Move gives a carried object to another member while the panel's view is
/// the roster or the stats (0x91, 0x90: the object picked by a press in the inventory view's
/// slot, which puts the mode back, `ui::panels::end_slot_press`) or the inventory (0x92), or
/// puts it into a bag the hero carries: the command loop's next 0x103f64 brings the inventory
/// view up to date, so the cells it keeps for a press in its slot no longer hold the object.
#[cfg(feature = "wide")]
#[test]
fn wide_move_of_a_carried_object_brings_the_inventory_view_up_to_date() {
    let Some(files) = files() else { return };
    let mut stale = Vec::new();
    for mode in [0x91, 0x90, 0x92] {
        let mut s = with_text(&files);
        let hero = Slot::from(s.actors.party.members[0]);
        let other = Slot::from(s.actors.party.members[1]);
        let obj = plain_carried(&s, hero);
        s.ui.panel.mode = mode;
        s.ui.panel.member = 0;
        s.ui.panel.container = hero as u16;
        s.ui.panel.dirty = true;
        let input = &mut s.ui.input;
        input.selection = obj;
        input.target_pending = 1;
        input.target_command = 0x87;
        input.targeting = 1;
        let kept = Rc::new(RefCell::new((false, false)));
        let out = Rc::clone(&kept);
        let cmd: Command = Box::new(move |st, io| {
            Box::pin(async move {
                crate::ui::panels::redraw_panel(&st, &io).await;
                let before = st.borrow().ui.panel.slots.inventory.cells.contains(&obj);
                super::push(&st, &io).await;
                crate::ui::panels::redraw_panel(&st, &io).await;
                let after = st.borrow().ui.panel.slots.inventory.cells.contains(&obj);
                *out.borrow_mut() = (before, after);
            })
        });
        let (e, lines) = run_engine(s, &files, &[InputEvent::Key(Key::Char('2'))], cmd);
        print_lines(&format!("Wide Move in mode {mode:#x}"), &lines);
        assert!(
            e.state().objects.parent(obj) == other,
            "mode {mode:#x}: moved"
        );
        let (before, after) = *kept.borrow();
        assert!(before, "mode {mode:#x}: the inventory view showed it");
        if after {
            stale.push(format!("{mode:#x}"));
        }
    }
    // Into a bag the hero carries, picked on the panel (Tab, Return: the inventory mode).
    let mut s = with_text(&files);
    let hero = Slot::from(s.actors.party.members[0]);
    let obj = plain_carried(&s, hero);
    let bag = bag_type(&s);
    let bag = crate::objects::create_in(&mut s, bag, crate::objects::IN_INVENTORY, 0, hero);
    assert!(bag >= 0, "a bag for the hero");
    s.ui.panel.mode = 0x92;
    s.ui.panel.member = 0;
    s.ui.panel.container = hero as u16;
    s.ui.panel.dirty = true;
    let k = listed_children(&mut s, hero)
        .iter()
        .position(|&c| c == bag)
        .filter(|&k| k < 12)
        .expect("the bag on the list's first page") as i8;
    let input = &mut s.ui.input;
    input.selection = obj;
    input.target_pending = 1;
    input.target_command = 0x87;
    input.targeting = 2;
    input.panel_cursor = (3 + k % 4, k / 4);
    let kept = Rc::new(RefCell::new((false, false)));
    let out = Rc::clone(&kept);
    let cmd: Command = Box::new(move |st, io| {
        Box::pin(async move {
            crate::ui::panels::redraw_panel(&st, &io).await;
            let before = st.borrow().ui.panel.slots.inventory.cells.contains(&obj);
            super::push(&st, &io).await;
            crate::ui::panels::redraw_panel(&st, &io).await;
            let after = st.borrow().ui.panel.slots.inventory.cells.contains(&obj);
            *out.borrow_mut() = (before, after);
        })
    });
    let (e, lines) = run_engine(s, &files, &TAB_RETURN, cmd);
    print_lines("Wide Move into the bag", &lines);
    assert!(e.state().objects.parent(obj) == bag, "into the bag: moved");
    let (before, after) = *kept.borrow();
    assert!(before, "into the bag: the inventory view showed it");
    if after {
        stale.push("into the bag".to_string());
    }
    assert!(
        stale.is_empty(),
        "the inventory view still shows it in {stale:?}"
    );
}

/// The weight test of Move to a member is `ble` (0x12b488): with his strength fixed, a member
/// whose strength × 20 equals his readied and carried weight plus the object's takes it; with
/// the object one weight unit heavier he refuses. The object's type weighs what makes the sum
/// that capacity, or one over it (a state change, the table read by 0x1110f6; he holds nothing
/// of that type, so his own sums do not move with it).
#[test]
fn move_to_a_member_whose_limit_equals_the_new_weight_takes_it() {
    let Some(files) = files() else { return };
    let mut s0 = with_text(&files);
    let hero = Slot::from(s0.actors.party.members[0]);
    let other = Slot::from(s0.actors.party.members[1]);
    let obj = plain_carried(&s0, hero);
    let kind = s0.objects.kind(obj);
    assert!(!super::seams::weighed_in_tenths(&s0, kind));
    let his = (0..crate::objects::OBJECT_SLOTS as Slot)
        .any(|o| s0.objects.kind(o) == kind && carried_by(&s0, other, o));
    assert!(!his, "he holds nothing of the object's type");
    let (equip, carried) = sums_of(&s0, other);
    let rest = equip + carried + super::seams::contents_weight(&s0, obj);
    let byte = 20 + (20 - rest.rem_euclid(20)) % 20;
    let capacity = rest + byte;
    let strength = capacity / 20;
    assert!(capacity % 20 == 0 && strength <= 0xff, "a reachable limit");
    s0.actors.attrs.strength[other as usize] = strength as u8;
    let keys = [InputEvent::Key(Key::Char('2'))];
    for (over, takes) in [(0, true), (1, false)] {
        let mut s = s0.clone();
        s.tables.tileflags.weight[usize::from(kind)] = (byte + over) as u8;
        let (equip, carried) = sums_of(&s, other);
        let total = equip
            + carried
            + super::seams::weight(&s, obj)
            + super::seams::contents_weight(&s, obj);
        assert!(total == capacity + over, "over by {over}");
        let (e, lines) = move_carried(s, &files, obj, hero, None, &keys);
        print_lines(&format!("Move over the limit by {over}"), &lines);
        let s = e.state().clone();
        let holder = if takes { other } else { hero };
        assert!(
            s.objects.parent(obj) == holder,
            "over by {over}: takes {takes}"
        );
        assert!(
            printed(&s0, &lines, 0x0012_8662) != takes,
            "over by {over}: the can't-carry line {}",
            !takes
        );
    }
}

/// Gold (0x10cef2 kind 4: its quantity is the whole quality / quantity word).
const GOLD: u16 = 0x58;

/// The direct children of `owner` of `kind`, quality `quality`: (slot, quantity byte or gold
/// word).
fn stacks_of(s: &State, owner: Slot, kind: u16, quality: u16) -> Vec<(Slot, u16)> {
    (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&o| {
            let ob = &s.objects;
            ob.place(o) != 0
                && ob.parent(o) == owner
                && ob.kind(o) == kind
                && (kind == GOLD || ob.qq_of(o) >> 8 == quality)
        })
        .map(|o| {
            let q = s.objects.qq_of(o);
            (o, if kind == GOLD { q } else { q & 0xff })
        })
        .collect()
}

/// The keys that type `n` at the how-many prompt (0x12a106) and end the line.
fn count_keys(n: &str) -> Vec<InputEvent> {
    n.chars()
        .map(|c| InputEvent::Key(Key::Char(c)))
        .chain([InputEvent::Key(Key::Enter)])
        .collect()
}

/// Move on carried stacks (0x10cef2 kinds 2 and 4): part of a quantity stack to another member
/// (0x10cf7c gives him a stack of the same quality, 0x10d16e takes the part from the holder,
/// 0x12b51c..0x12b58a); a whole quantity stack into a bag (taken from the stack itself: all of
/// it, so removed, 0x10c1d6, then given to the bag, 0x12b928..0x12b9d8); part of a gold word
/// out of the bag to its holder (the word less the part, 0x12b848..0x12b8b2).
#[test]
fn move_splits_carried_stacks_by_their_count() {
    let Some(files) = files() else { return };
    let mut s0 = with_text(&files);
    let hero = Slot::from(s0.actors.party.members[0]);
    let other = Slot::from(s0.actors.party.members[1]);
    let gold = GOLD;
    let kind = *s0
        .objects
        .defs
        .stackable
        .iter()
        .find(|&&k| k != gold && k != 0x5a && crate::objects::stack_kind(&s0.objects, k) == 2)
        .expect("a quantity stack type");
    let (quality, make) = (0x37, crate::objects::create_in);
    let part = make(
        &mut s0,
        kind,
        crate::objects::IN_INVENTORY,
        quality << 8 | 10,
        hero,
    );
    assert!(part >= 0);
    let total = |s: &State, owner| -> u16 {
        stacks_of(s, owner, kind, quality)
            .iter()
            .map(|&(_, n)| n)
            .sum()
    };
    let theirs = total(&s0, other);
    let mut keys = count_keys("3");
    keys.push(InputEvent::Key(Key::Char('2')));
    let (e, lines) = move_carried(s0.clone(), &files, part, hero, None, &keys);
    print_lines("Move 3 of a stack to the second member", &lines);
    let s1 = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(
        s1.objects.qq_of(part) == quality << 8 | 7,
        "the stack keeps 7"
    );
    assert!(s1.objects.parent(part) == hero, "with the hero");
    assert!(total(&s1, other) == theirs + 3, "he has 3 more");
    assert!(ended(&s1));

    let mut s1 = s1;
    let bag = bag_type(&s1);
    let bag = make(&mut s1, bag, crate::objects::IN_INVENTORY, 0, hero);
    let whole = make(
        &mut s1,
        kind,
        crate::objects::IN_INVENTORY,
        quality << 8 | 5,
        hero,
    );
    assert!(bag >= 0 && whole >= 0);
    let mut keys = count_keys("");
    keys.extend(TAB_RETURN);
    let (e, lines) = move_carried(s1.clone(), &files, whole, hero, Some(bag), &keys);
    print_lines("Move a whole stack into the bag", &lines);
    let s2 = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(printed(&s1, &lines, 0x0012_87d0), "the into line");
    let inside = stacks_of(&s2, bag, kind, quality);
    assert!(inside.len() == 1 && inside[0].1 == 5, "the bag holds the 5");
    let left: Vec<Slot> = stacks_of(&s2, hero, kind, quality)
        .iter()
        .map(|&(o, _)| o)
        .collect();
    assert!(left == [part], "the hero keeps only the first stack");
    assert!(ended(&s2));

    let mut s2 = s2;
    let coins = make(&mut s2, gold, crate::objects::IN_CONTAINER, 300, bag);
    assert!(coins >= 0);
    let held = total_gold(&s2, hero);
    let mut keys = count_keys("100");
    keys.extend(TAB_RETURN);
    let (e, lines) = move_carried(s2.clone(), &files, coins, bag, Some(bag), &keys);
    print_lines("Move 100 gold out of the bag", &lines);
    let s3 = e.state().clone();
    assert!(e.stub_hits().is_empty(), "no stub");
    assert!(printed(&s2, &lines, 0x0012_87c8), "the out-of line");
    assert!(s3.objects.qq_of(coins) == 200, "the bag's gold less 100");
    assert!(s3.objects.parent(coins) == bag, "still in the bag");
    assert!(total_gold(&s3, hero) == held + 100, "the hero has 100 more");
    assert!(ended(&s3));
}

/// The gold word of `owner`'s direct gold children.
fn total_gold(s: &State, owner: Slot) -> u16 {
    stacks_of(s, owner, GOLD, 0).iter().map(|&(_, n)| n).sum()
}

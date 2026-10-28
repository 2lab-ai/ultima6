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

// ---- spec 17: the context menu --------------------------------------------------------------

use crate::ui::menu::{self, Menu, Reason, Run, Step};

/// The menu of a right press at frame pixel `at` with the prompt up (A4-0x7a2c set on a copy of
/// `s`); the query leaves that state as it was, its print queues (the queued text-layer calls,
/// combat's said lines) included, and draws nothing (the `Io`'s back buffer mark, its queue).
fn menu_of(s: &State, files: &Files, at: (i16, i16)) -> Option<Menu> {
    let mut prompt = s.clone();
    prompt.main.reading = 1;
    let before = prompt.clone();
    let io = Io::new(files.clone());
    let (mark, events) = (io.back_mark(), io.pending_events());
    let m = menu::query(&prompt, &io, at.0, at.1);
    assert!(
        prompt.main.pending_print == before.main.pending_print,
        "no text queued"
    );
    assert!(prompt.combat.said == before.combat.said, "no line said");
    assert!(prompt == before, "the query leaves the state as it was");
    assert_eq!(
        (io.back_mark(), io.pending_events()),
        (mark, events),
        "nothing drawn or queued"
    );
    m
}

/// The codes of `object`'s section, in order.
fn codes(m: &Menu, object: Slot) -> Vec<u16> {
    m.sections
        .iter()
        .find(|s| s.object == object)
        .map_or_else(Vec::new, |s| s.entries.iter().map(|e| e.code).collect())
}

/// The reason of `code` in `object`'s section (`Some(None)`: enabled; `None`: not shown).
fn reason(m: &Menu, object: Slot, code: u16) -> Option<Option<Reason>> {
    m.entry(object, code).map(|e| e.reason)
}

/// The command loop on `s` at its prompt (A4-0x7a2c set, nothing queued).
fn prompt_engine(s: State, files: &Files) -> Engine {
    let mut e = Engine::with_game(s, files.clone(), |st, io| async move {
        crate::main_loop::commands::command_loop(&st, &io).await;
    });
    for _ in 0..400 {
        if at_prompt(&e, &[]) {
            return e;
        }
        e.tick(&[], 16);
    }
    panic!("the prompt never came up");
}

/// `u6-web`'s `Game::at_prompt`: the loop reads a command with nothing queued.
fn at_prompt(e: &Engine, queued: &[InputEvent]) -> bool {
    e.state().main.reading == 1 && e.io().pending_events() == 0 && queued.is_empty()
}

/// The quiet the engine's context menu steps a run with (`ui::popup::Popup::input_with`): nothing
/// queued for the game (the engine's own test lets the repeats the command prompt takes back
/// through; none is queued here), and every event of the tick's input inert
/// ([`crate::ui::popup::inert`]: a button-less move, a release, a repeat the prompt takes back).
fn quiet(e: &Engine, queued: &[InputEvent]) -> bool {
    let prompt = crate::engine::command_prompt(&e.state());
    e.io().pending_events() == 0 && queued.iter().all(|ev| crate::ui::popup::inert(prompt, ev))
}

/// One step as the engine's context menu runs a chosen entry with `run` held
/// (`ui::popup::Popup::input_with`): the run's step before the tick, its target press after the
/// tick's events, then the tick (16 ms) with `queued`. True when the target press went in at this
/// step.
fn page_step(e: &mut Engine, queued: &mut Vec<InputEvent>, run: &mut Option<Run>) -> bool {
    let mut released = false;
    if let Some(r) = *run {
        match r.step(&e.state(), quiet(e, queued)) {
            Step::Hold => {}
            Step::Release(press) => {
                queued.extend(press);
                *run = None;
                released = true;
            }
            Step::Drop => *run = None,
        }
    }
    e.tick(queued, 16);
    queued.clear();
    released
}

/// Runs entry `code` on the target at `at` as the engine's context menu runs a chosen entry
/// (spec 17 Running a choice, `ui::popup`: the icon's press, then the target press once the game
/// reads it), until the prompt is up again with no run left (at most 3000 steps); a later prompt
/// of the command (a direction, the next weapon's target) is answered with Escape. True when the
/// target press went in.
fn choose(e: &mut Engine, code: u16, at: (i16, i16)) -> bool {
    choose_then(e, code, at, &[])
}

/// [`choose`], the command's later prompts answered with `then` first, one event list each
/// (a direction key, a press on a map cell), then Escape.
fn choose_then(e: &mut Engine, code: u16, at: (i16, i16), then: &[&[InputEvent]]) -> bool {
    let (icon, run) = Run::start(&e.state(), code, at).expect("a command with a target");
    let (mut queued, mut run) = (icon.to_vec(), Some(run));
    let mut then = then.iter();
    let mut released = false;
    for _ in 0..3000 {
        let waits = e.state().ui.input.in_wait && e.state().main.reading == 0;
        if run.is_none() && waits && quiet(e, &queued) && queued.is_empty() {
            match then.next() {
                Some(events) => queued.extend_from_slice(events),
                None => queued.push(InputEvent::Key(Key::Escape)),
            }
        }
        released |= page_step(e, &mut queued, &mut run);
        if run.is_none() && at_prompt(e, &queued) {
            return released;
        }
    }
    let st = e.state();
    let i = &st.ui.input;
    panic!(
        "the command did not come back to the prompt: reading {} wait {} pending {} cmd {:#x} accept {} queued {} released {released}",
        st.main.reading,
        i.in_wait,
        i.target_pending,
        i.target_command,
        i.accept,
        e.io().pending_events()
    );
}

/// The nearest closed steel door (0x12c, frames 4..7: closed, neither locked 8..11 nor magically
/// locked 12..) with a tile beside it to stand on.
pub(crate) fn closed_door(s: &State) -> (Slot, (u16, u16)) {
    nearest(s, 0x12c, |s, o| s.objects.tf(o) >> 10 & 0xc == 4)
        .into_iter()
        .find_map(|o| Some((o, beside(s, o)?)))
        .expect("a closed door near the start")
}

fn at_slot(s: &State, slot: Slot) -> (i16, i16) {
    cell_pixel(s, s.objects.x(slot), s.objects.y(slot))
}

/// Spec 17 Acceptance 1: a right press on a closed door next to the leader gives one section,
/// the door's, with Look and Use enabled and Attack last; the query changes nothing. Choosing Use
/// (its icon, then the press at the door once Use reads its target) opens the door and leaves the
/// same state and RNG as `u` then a left press on the door, tick for tick; no underline is
/// written. The one difference is by design and pinned: the letter starts the cross-hair
/// (0x104216: the leader's cell, range -1), the icon leaves it as it was (0x10658e's code goes
/// straight to the command); the next input (a step) then does the same in both.
#[test]
fn a_door_next_to_the_leader_offers_use_and_use_opens_it_as_u_and_a_press() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let at = at_slot(&s, door);
    let m = menu_of(&s, &files, at).expect("a menu on the door");
    assert!(
        m.kind == menu::Kind::Map && m.sections.len() == 1,
        "one section"
    );
    assert_eq!(m.sections[0].object, door, "the door's");
    assert!(!m.sections[0].name.is_empty(), "named");
    let door_codes = codes(&m, door);
    eprintln!("door entries: {}", door_codes.len());
    assert_eq!(reason(&m, door, menu::LOOK), Some(None), "Look enabled");
    assert_eq!(reason(&m, door, menu::USE), Some(None), "Use enabled");
    assert_eq!(door_codes.last(), Some(&menu::ATTACK), "Attack last");

    let closed = s.objects.tf(door) >> 10;
    let underline = crate::ui::globals::last_command(&s.actors);
    let (cursor, range) = (s.ui.input.cursor, s.ui.input.range);
    let (mut by_letter, mut by_menu) = letter_and_menu(&s, &files, 'u', menu::USE, at);
    let (a, b) = (by_letter.state().clone(), by_menu.state().clone());
    assert_eq!(a.objects.tf(door) >> 10, closed ^ 4, "the door opened");
    assert_eq!(b.objects.tf(door) >> 10, closed ^ 4, "the menu opened it");
    assert!(a.rng == b.rng, "the same RNG");
    assert_eq!(
        crate::ui::globals::last_command(&b.actors),
        underline,
        "no underline written"
    );
    let leader_cell = crate::ui::target::LEADER_CELL;
    assert_eq!(
        (a.ui.input.cursor, a.ui.input.range),
        (leader_cell, -1),
        "the letter's cross-hair"
    );
    assert_eq!(
        (b.ui.input.cursor, b.ui.input.range),
        (cursor, range),
        "the icon's: as it was"
    );
    let mut b2 = b.clone();
    (b2.ui.input.cursor, b2.ui.input.range) = (a.ui.input.cursor, a.ui.input.range);
    assert!(a == b2, "the same state but for the cross-hair's start");
    let step = [Key::Right, Key::Left, Key::Down, Key::Up]
        .into_iter()
        .find(|&k| {
            let (dx, dy) = match k {
                Key::Right => (1, 0),
                Key::Left => (-1, 0),
                Key::Down => (0, 1),
                _ => (0, -1),
            };
            walkable(&a, i32::from(bx) + dx, i32::from(by) + dy)
        })
        .expect("a free tile beside the leader");
    for e in [&mut by_letter, &mut by_menu] {
        e.tick(&[InputEvent::Key(step)], 16);
        for _ in 0..400 {
            if at_prompt(e, &[]) {
                break;
            }
            e.tick(&[], 16);
        }
    }
    let (a, mut b) = (by_letter.state().clone(), by_menu.state().clone());
    assert_ne!(leader_at(&a), (bx, by), "the step walked");
    (b.ui.input.cursor, b.ui.input.range) = (a.ui.input.cursor, a.ui.input.range);
    assert!(a == b, "the next input does the same in both");
}

/// A walkable tile `d` cells from `slot` along a row or a column (east, west, south, north),
/// else anywhere on the ring `d` cells off on the farther axis.
fn tile_at_distance(s: &State, slot: Slot, d: i32) -> Option<(u16, u16)> {
    let (x, y) = (i32::from(s.objects.x(slot)), i32::from(s.objects.y(slot)));
    let ring = (-d..=d).flat_map(|a| [(a, -d), (a, d), (-d, a), (d, a)]);
    [(d, 0), (-d, 0), (0, d), (0, -d)]
        .into_iter()
        .chain(ring)
        .map(|(dx, dy)| (x + dx, y + dy))
        .find(|&(x, y)| walkable(s, x, y))
        .map(|(x, y)| (x as u16, y as u16))
}

/// Spec 17 Acceptance 2: a right press on a door three cells from the leader shows Use, disabled
/// as far; the query prints nothing and passes no turn (the state is unchanged). Use itself, run
/// on that target, says it is out of range and opens nothing (0x1305e0's distance test,
/// `usecmd::dispatch::out_of_reach`).
#[test]
fn a_door_three_cells_away_shows_use_disabled_as_far() {
    let Some(files) = files() else { return };
    let s0 = with_text(&files);
    let (door, (sx, sy)) = nearest(&s0, 0x12c, |s, o| s.objects.tf(o) >> 10 & 4 != 0)
        .into_iter()
        .find_map(|o| Some((o, tile_at_distance(&s0, o, 3)?)))
        .expect("a closed door with a tile three cells off");
    let mut s = s0.clone();
    stand_at(&mut s, sx, sy);
    let at = at_slot(&s, door);
    let m = menu_of(&s, &files, at).expect("a menu");
    assert_eq!(reason(&m, door, menu::USE), Some(Some(Reason::Far)));
    assert_eq!(reason(&m, door, menu::LOOK), Some(None));
    let closed = s.objects.tf(door) >> 10;
    let mut e = prompt_engine(s, &files);
    assert!(choose(&mut e, menu::USE, at), "Use read its target");
    assert_eq!(e.state().objects.tf(door) >> 10, closed, "nothing opened");
}

/// The hero's inventory panel as its draw leaves it (mode 0x92, member 0, its list cells), and
/// the frame pixel of list cell `k`.
fn show_inventory(s: &mut State) -> Vec<Slot> {
    let hero = Slot::from(s.actors.party.members[0]);
    s.ui.panel.mode = 0x92;
    s.ui.panel.member = 0;
    s.ui.panel.container = hero as u16;
    crate::inventory::recount(s, hero);
    let listed = listed_children(s, hero);
    for (k, cell) in s.inventory.cells[..12].iter_mut().enumerate() {
        *cell = listed.get(k).copied().unwrap_or(0);
    }
    listed
}

fn list_cell(k: usize) -> (i16, i16) {
    let k = k as i16;
    (0xf8 + (k % 4) * 16 + 8, 0x20 + (k / 4) * 16 + 8)
}

/// The frame pixel of equipment slot `slot` (0x111eec's cells).
fn equipment_cell(slot: i16) -> (i16, i16) {
    match slot {
        0 => (0xc8 + 8, 0x10 + 8),
        1..=3 => (0xb0 + 8, 0x18 + (slot - 1) * 16 + 8),
        4..=6 => (0xe0 + 8, 0x18 + (slot - 4) * 16 + 8),
        _ => (0xc8 + 8, 0x40 + 8),
    }
}

/// The hero's readied spellbooks taken away.
fn without_books(s: &mut State) {
    for b in crate::magic::cast::readied_books(s) {
        crate::objects::remove(s, b);
    }
}

/// The leader's map position.
fn leader_at(s: &State) -> (u16, u16) {
    let l = leader_slot(s);
    (s.objects.x(l), s.objects.y(l))
}

fn leader_slot(s: &State) -> Slot {
    let p = &s.actors.party;
    Slot::from(p.members[usize::from(p.leader_index)])
}

/// Spec 17 Acceptance 5: without a spellbook Cast is shown disabled (book), on a door in view.
/// Forced through the run anyway on a free cell two steps off, its icon press goes in, Cast refuses before it reads a target, and the held
/// target press is dropped: the party does not walk, no underline is written. A target press
/// queued with the icon at once instead reaches the prompt as a click on the map and walks.
#[test]
fn cast_without_a_book_is_disabled_and_forced_its_target_press_is_dropped() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    without_books(&mut s);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let (lx, ly) = leader_at(&s);
    let (tx, ty) = [(2i32, 0i32), (-2, 0), (0, 2), (0, -2)]
        .into_iter()
        .map(|(dx, dy)| (i32::from(lx) + dx, i32::from(ly) + dy))
        .find(|&(x, y)| {
            walkable(&s, x, y) && walkable(&s, (x + i32::from(lx)) / 2, (y + i32::from(ly)) / 2)
        })
        .expect("a free cell two steps off");
    let at = cell_pixel(&s, tx as u16, ty as u16);
    let m = menu_of(&s, &files, at_slot(&s, door)).expect("a menu");
    assert_eq!(reason(&m, door, menu::CAST), Some(Some(Reason::Book)));
    let underline = crate::ui::globals::last_command(&s.actors);

    let mut e = prompt_engine(s.clone(), &files);
    assert!(!choose(&mut e, menu::CAST, at), "the target press dropped");
    assert_eq!(leader_at(&e.state()), (lx, ly), "the party did not walk");
    assert_eq!(
        crate::ui::globals::last_command(&e.state().actors),
        underline
    );

    let mut naive = prompt_engine(s, &files);
    let mut queued = menu::icon_press(menu::CAST).to_vec();
    queued.extend(menu::left_press(at.0, at.1));
    let mut none = None;
    for _ in 0..600 {
        page_step(&mut naive, &mut queued, &mut none);
    }
    assert_ne!(
        leader_at(&naive.state()),
        (lx, ly),
        "a press queued at once walks"
    );
}

/// `s` with the target a left press at `at` confirms (0x105f7c on the pointer there), as the
/// command's handler gets it.
fn confirmed(s: &State, at: (i16, i16)) -> State {
    let mut c = s.clone();
    c.ui.input.pointer = at;
    crate::ui::cursor::target_from_pointer(&mut c);
    c
}

fn get_cmd() -> Command {
    Box::new(|st, io| Box::pin(async move { super::get(&st, &io).await }))
}

fn use_cmd() -> Command {
    Box::new(|st, io| Box::pin(async move { crate::usecmd::use_command(&st, &io).await }))
}

/// `obj` still lies on the map where it lay in `before`, as the same type.
fn still_there(before: &State, after: &State, obj: Slot) -> bool {
    let (b, a) = (&before.objects, &after.objects);
    a.type_frame[obj as usize] == b.type_frame[obj as usize]
        && a.place(obj) == 0
        && (a.x(obj), a.y(obj)) == (b.x(obj), b.y(obj))
}

/// An entry of a right press: its frame pixel, the object of its section, the command.
#[derive(Clone, Copy)]
struct Pick((i16, i16), Slot, u16);

/// The menu's `code` entry on the object at `at` (`object`) against the real handler `cmd` run
/// on that target: enabled exactly when `reached` holds after it (the entry must be shown).
fn agrees(
    label: &str,
    s: &State,
    files: &Files,
    Pick(at, object, code): Pick,
    keys: &[InputEvent],
    cmd: Command,
    reached: impl Fn(&State, &State) -> bool,
) -> Option<Reason> {
    let m = menu_of(s, files, at).expect("a menu");
    let entry = m
        .entry(object, code)
        .unwrap_or_else(|| panic!("{label}: the entry is shown"));
    let before = confirmed(s, at);
    let (after, _) = run(before.clone(), files, keys, cmd);
    let did = reached(&before, &after);
    eprintln!(
        "{label}: enabled {}, reached {}",
        u8::from(entry.enabled()),
        u8::from(did)
    );
    assert_eq!(entry.enabled(), did, "{label}");
    entry.reason
}

/// Get's checks (0x12a7a4) as the menu reads them, each against the real Get on a copy: far
/// (two tiles off), too heavy (no strength), no free hand (a lit torch, both hands full with the
/// leader's panel shown), and the theft mark against the karma Get takes.
#[test]
fn gets_far_heavy_and_hands_checks_agree_with_get() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (bread, (sx, sy)) = nearest(&s, 0x80, |s, o| beside(s, o).is_some())
        .into_iter()
        .find_map(|o| Some((o, beside(&s, o)?)))
        .expect("a loaf to stand by");
    stand_at(&mut s, sx, sy);
    let at = at_slot(&s, bread);
    let left = |b: &State, a: &State| !still_there(b, a, bread);
    let hero = leader_slot(&s);
    let r = agrees(
        "Get beside",
        &s,
        &files,
        Pick(at, bread, menu::GET),
        &[],
        get_cmd(),
        left,
    );
    assert_eq!(r, None);
    let m = menu_of(&s, &files, at).expect("a menu");
    let theft = m.entry(bread, menu::GET).expect("Get").theft;
    let (after, _) = run(confirmed(&s, at), &files, &[], get_cmd());
    assert_eq!(
        theft,
        after.actors.karma < s.actors.karma,
        "theft ⇔ karma lost"
    );

    let mut weak = s.clone();
    weak.actors.attrs.strength[hero as usize] = 0;
    let r = agrees(
        "Get, too heavy",
        &weak,
        &files,
        Pick(at, bread, menu::GET),
        &[],
        get_cmd(),
        left,
    );
    assert_eq!(r, Some(Reason::Heavy));

    let far = tile_at_distance(&s, bread, 2).expect("a tile two off");
    let mut off = s.clone();
    stand_at(&mut off, far.0, far.1);
    let at_off = at_slot(&off, bread);
    let r = agrees(
        "Get, far",
        &off,
        &files,
        Pick(at_off, bread, menu::GET),
        &[],
        get_cmd(),
        left,
    );
    assert_eq!(r, Some(Reason::Far));

    let (tx, ty) = beside_tile(&s, sx, sy).expect("a free tile by the hero");
    let mut lit = s.clone();
    let z = u16::from(lit.map.centre.z);
    let torch = crate::objects::create_on_map(&mut lit.objects, 0x5a | 1 << 10, 0, 1, tx, ty, z);
    crate::map::view::update_view(&mut lit);
    lit.ui.panel.member = i16::from(lit.actors.party.leader_index);
    let at_torch = at_slot(&lit, torch);
    let took = |b: &State, a: &State| !still_there(b, a, torch);
    let r = agrees(
        "Get a torch",
        &lit,
        &files,
        Pick(at_torch, torch, menu::GET),
        &[],
        get_cmd(),
        took,
    );
    assert_eq!(r, None);
    lit.inventory.set_equip(2, bread);
    lit.inventory.set_equip(5, bread);
    let r = agrees(
        "Get, no hand",
        &lit,
        &files,
        Pick(at_torch, torch, menu::GET),
        &[],
        get_cmd(),
        took,
    );
    assert_eq!(r, Some(Reason::Hands));
}

/// The leader's movement points went down by `n`.
fn spent(before: &State, after: &State, n: u8) -> bool {
    leader_mp(before).wrapping_sub(leader_mp(after)) == n
}

/// Use's checks (0x1305e0) as the menu reads them, each against the real Use on a copy: a door
/// next to the leader (opened) or two tiles off (far), a door under a townsperson standing on
/// it (the door test), a brazier whose frame refuses (its case), and a shovel readied by
/// another member than the one the panel shows (in another's hand) against the leader's own.
/// "Reached" is the switch running (the 5 movement points of the end step) for the checks
/// before it, the object's own change for a case.
#[test]
fn uses_far_door_hand_and_case_checks_agree_with_use() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let at = at_slot(&s, door);
    let ran = |b: &State, a: &State| spent(b, a, 5);
    let r = agrees(
        "Use beside",
        &s,
        &files,
        Pick(at, door, menu::USE),
        &[],
        use_cmd(),
        ran,
    );
    assert_eq!(r, None);
    let far = tile_at_distance(&s, door, 2).expect("a tile two off the door");
    let mut off = s.clone();
    stand_at(&mut off, far.0, far.1);
    let at_off = at_slot(&off, door);
    let r = agrees(
        "Use, far",
        &off,
        &files,
        Pick(at_off, door, menu::USE),
        &[],
        use_cmd(),
        ran,
    );
    assert_eq!(r, Some(Reason::Far));

    let mut blocked = s.clone();
    let npc = (1..FIRST_TEMP_ACTOR)
        .find(|&a| {
            let o = &blocked.objects;
            o.type_frame[a as usize] != 0
                && o.place(a) == 0
                && blocked.actors.attrs.status[a as usize] & 0x80 == 0
        })
        .expect("a townsperson");
    let (dx, dy, dz) = (
        blocked.objects.x(door),
        blocked.objects.y(door),
        u16::from(blocked.objects.z(door)),
    );
    crate::objects::move_to(&mut blocked, npc, dx, dy, dz);
    crate::map::view::update_view(&mut blocked);
    let r = agrees(
        "Use the door under a townsperson",
        &blocked,
        &files,
        Pick(at, door, menu::USE),
        &[],
        use_cmd(),
        ran,
    );
    assert_eq!(r, Some(Reason::Door), "the townsperson on the door");

    let brazier = find(&s, 0xce);
    let (zx, zy) = beside(&s, brazier).expect("a tile by the brazier");
    let mut lamp = s.clone();
    stand_at(&mut lamp, zx, zy);
    let at_lamp = at_slot(&lamp, brazier);
    let changed = |b: &State, a: &State| a.objects.tf(brazier) != b.objects.tf(brazier);
    for bit in [0u16, 2] {
        let mut l = lamp.clone();
        let t = l.objects.tf(brazier);
        l.objects.type_frame[brazier as usize] = (t & 0x3ff) | (((t >> 10) & !2 | bit) << 10);
        let label = format!("Use the brazier, frame bit 1 {}", bit != 0);
        let r = agrees(
            &label,
            &l,
            &files,
            Pick(at_lamp, brazier, menu::USE),
            &[],
            use_cmd(),
            changed,
        );
        assert_eq!(r, (bit != 0).then_some(Reason::Case));
    }

    for member in [0i16, 1] {
        let mut h = s.clone();
        let actor = Slot::from(h.actors.party.members[member as usize]);
        let readied: Vec<Slot> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
            .filter(|&o| {
                h.objects.place(o) == crate::objects::READIED && h.objects.parent(o) == actor
            })
            .collect();
        for o in readied {
            crate::objects::remove(&mut h, o);
        }
        let shovel = crate::objects::create_in(&mut h, 0x67, crate::objects::READIED, 0, actor);
        assert!(shovel >= 0);
        h.ui.panel.mode = 0x92;
        h.ui.panel.member = member;
        h.ui.panel.container = actor as u16;
        crate::inventory::recount(&mut h, actor);
        let k = (0..8)
            .find(|&k| h.inventory.equip(k) == shovel)
            .expect("the shovel readied");
        let label = format!("Use a shovel of member {member}");
        let r = agrees(
            &label,
            &h,
            &files,
            Pick(equipment_cell(k), shovel, menu::USE),
            &[],
            use_cmd(),
            ran,
        );
        assert_eq!(r, (member != 0).then_some(Reason::Hand));
    }
}

/// Spec 17 round 3, item 19: a locked door (frames 8..11), a magically locked one (12..) and a
/// chest on the map locked (frame 2) or magically locked (3): Use is disabled as locked (one
/// predicate beside the door and chest cases, `world::locked`, the cases' own frame tests), and
/// run anyway the handler is the original's: the locked line (0x128854), the magically-locked
/// line (0x12882a) or the no-effect message (A4-0x6c64), the frame unchanged, the end-of-use
/// 5 movement points spent.
#[test]
fn a_locked_door_or_chest_disables_use_as_locked_and_use_still_gives_its_line() {
    use crate::usecmd::defs;
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let io = Io::new(files.clone());
    let no_effect =
        crate::main_loop::pointed_string(&io, crate::image::A4.wrapping_sub(defs::MSG_NO_EFFECT));
    let locked_line = super::text::string(&s, 0x0012_8854);
    let magic_line = super::text::string(&s, 0x0012_882a);
    let (lx, ly) = leader_at(&s);
    let (cx, cy) = beside_tile(&s, lx, ly).expect("a free tile by the leader");
    let z = u16::from(s.map.centre.z);
    let half = s.objects.tf(door) >> 10 & 3;
    let with_frame = |obj: Slot, f: u16| {
        let mut st = s.clone();
        let t = st.objects.tf(obj) & 0x3ff;
        st.objects.type_frame[obj as usize] = t | f << 10;
        crate::map::view::update_view(&mut st);
        (st, obj)
    };
    let chest = |f: u16| {
        let mut st = s.clone();
        let c = crate::objects::create_on_map(&mut st.objects, 0x62 | f << 10, 0, 0, cx, cy, z);
        assert!(c >= 0);
        crate::map::view::update_view(&mut st);
        (st, c)
    };
    for (label, (st, obj), line) in [
        ("a locked door", with_frame(door, half | 8), &locked_line),
        (
            "a magically locked door",
            with_frame(door, half | 12),
            &magic_line,
        ),
        ("a locked chest", chest(2), &no_effect),
        ("a magically locked chest", chest(3), &no_effect),
    ] {
        let at = at_slot(&st, obj);
        let m = menu_of(&st, &files, at).expect("a menu");
        let r = reason(&m, obj, menu::USE).expect("Use shown");
        let mut e = prompt_engine(st.clone(), &files);
        let before = lines_of(&e.state()).len();
        choose(&mut e, menu::USE, at);
        let after = e.state().clone();
        let lines = lines_of(&after);
        let said = said_line(&lines[before.saturating_sub(1).min(lines.len())..], line);
        let same = after.objects.tf(obj) == st.objects.tf(obj);
        let ran = spent(&st, &after, 5);
        eprintln!(
            "{label}: enabled {}, its line {}, frame kept {}, the end step {}",
            u8::from(r.is_none()),
            u8::from(said),
            u8::from(same),
            u8::from(ran)
        );
        assert_eq!(r, Some(Reason::Locked), "{label}: the menu");
        assert!(
            said && same && ran,
            "{label}: the handler as the original's"
        );
    }
}

/// Move's checks (0x12ae04) as the menu reads them, against the real Move on a copy (the
/// direction south): a chair next to the view centre moves, one two tiles off is far; a
/// townsperson lying down never moves, the same one standing moves when the strength contest
/// lets it (a seed where it does, searched).
#[test]
fn moves_far_and_lying_checks_agree_with_move() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let free = |s: &State, o: Slot, dx: i32, dy: i32| {
        walkable(
            s,
            i32::from(s.objects.x(o)) + dx,
            i32::from(s.objects.y(o)) + dy,
        )
    };
    let chair = *nearest(&s, 0xfc, |s, o| {
        free(s, o, 0, 1) && free(s, o, 1, 0) && free(s, o, 2, 0)
    })
    .first()
    .expect("a chair with room");
    let (cx, cy) = (s.objects.x(chair), s.objects.y(chair));
    let south = [InputEvent::Key(Key::Down)];
    let moved = |b: &State, a: &State| !still_there(b, a, chair);
    stand_at(&mut s, cx + 1, cy);
    let at = at_slot(&s, chair);
    let r = agrees(
        "Move beside",
        &s,
        &files,
        Pick(at, chair, menu::MOVE),
        &south,
        push_cmd(),
        moved,
    );
    assert_eq!(r, None);
    let mut off = s.clone();
    stand_at(&mut off, cx + 2, cy);
    let at_off = at_slot(&off, chair);
    let r = agrees(
        "Move, far",
        &off,
        &files,
        Pick(at_off, chair, menu::MOVE),
        &south,
        push_cmd(),
        moved,
    );
    assert_eq!(r, Some(Reason::Far));

    let npc = (1..FIRST_TEMP_ACTOR)
        .find(|&a| {
            let o = &s.objects;
            o.type_frame[a as usize] != 0
                && o.place(a) == 0
                && o.kind(a) != 0x153
                && s.actors.attrs.status[a as usize] & 0x80 == 0
                && !super::push::immovable(&s, a)
        })
        .expect("a townsperson");
    let z = u16::from(s.map.centre.z);
    let mut standing = s.clone();
    crate::objects::move_to(&mut standing, npc, cx, cy, z);
    let t = standing.objects.tf(npc);
    standing.objects.type_frame[npc as usize] = t & !(3 << 10);
    crate::map::view::update_view(&mut standing);
    let at_npc = at_slot(&standing, npc);
    let shoved = |b: &State, a: &State| !still_there(b, a, npc);
    let seed = (0..200u16)
        .find(|&seed| {
            let mut t = standing.clone();
            t.rng.seed(seed);
            let (after, _) = run(confirmed(&t, at_npc), &files, &south, push_cmd());
            shoved(&t, &after)
        })
        .expect("a seed the contest lets through");
    standing.rng.seed(seed);
    let r = agrees(
        "Move a standing townsperson",
        &standing,
        &files,
        Pick(at_npc, npc, menu::MOVE),
        &south,
        push_cmd(),
        shoved,
    );
    assert_eq!(r, None);
    let mut lying = standing.clone();
    lying.objects.type_frame[npc as usize] = t | 3 << 10;
    let r = agrees(
        "Move one lying down",
        &lying,
        &files,
        Pick(at_npc, npc, menu::MOVE),
        &south,
        push_cmd(),
        shoved,
    );
    assert_eq!(r, Some(Reason::Lying));
}

/// A readied spellbook for the hero holding spell `spell` (its current spell), with 5 of each
/// reagent it takes; the books before it taken away; the hero's level and magic points up.
fn with_book(s: &mut State, spell: u8) -> Slot {
    without_books(s);
    let hero = leader_slot(s);
    let q = u16::from(spell) << 8;
    let book =
        crate::objects::create_in(s, crate::magic::SPELLBOOK, crate::objects::READIED, q, hero);
    let page = crate::objects::create_in(
        s,
        crate::magic::SPELL,
        crate::objects::IN_CONTAINER,
        q,
        book,
    );
    assert!(book >= 0 && page >= 0);
    let mask = s.magic.tables.mask(usize::from(spell));
    for i in 0..8 {
        if mask >> i & 1 != 0 {
            let kind = s.magic.tables.reagents[i];
            crate::objects::create_in(s, kind, crate::objects::IN_INVENTORY, 5, hero);
        }
    }
    s.actors.attrs.level[hero as usize] = 8;
    s.actors.attrs.magic[hero as usize] = 40;
    book
}

/// Cast at a map target (0x11abd0 pseudocode l.41-49) as the menu reads it, against the real
/// Cast run from the prompt as the menu runs it (the icon, then the press on the cell once the
/// book reads it): a book holding a first-circle spell that takes reagents casts it (its magic
/// point spent); no book, the negate timer, a book of no spell, too low a level, no magic points
/// and no reagents each refuse, as the menu says.
#[test]
fn casts_checks_at_a_target_agree_with_cast() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let at = at_slot(&s, door);
    let hero = leader_slot(&s);
    let points = move |b: &State, a: &State| {
        a.actors.attrs.magic[hero as usize] < b.actors.attrs.magic[hero as usize]
    };
    // The first first-circle spell that takes reagents and casts on a cell without a prompt of
    // its own (a timed spell: 0x09 is the first of them when it takes any).
    let spell = [0x09u8, 0x07, 0x00, 0x05, 0x06, 0x03]
        .into_iter()
        .find(|&sp| s.magic.tables.mask(usize::from(sp)) != 0)
        .expect("a first-circle spell with reagents");
    let mut base = s.clone();
    let book = with_book(&mut base, spell);
    let mask = base.magic.tables.mask(usize::from(spell));
    assert_ne!(mask, 0, "the spell takes reagents");
    let mut cases: Vec<(&str, State, Option<Reason>)> = vec![("a book", base.clone(), None)];
    let mut none = base.clone();
    without_books(&mut none);
    cases.push(("no book", none, Some(Reason::Book)));
    let mut negate = base.clone();
    negate.actors.timers[13] = 5;
    cases.push(("the negate timer", negate, Some(Reason::Negate)));
    let mut empty = base.clone();
    let spell = crate::objects::first_child(&mut empty.objects, book);
    crate::objects::remove(&mut empty, spell);
    cases.push(("an empty book", empty, Some(Reason::Spells)));
    let mut low = base.clone();
    low.actors.attrs.level[hero as usize] = 0;
    cases.push(("level 0", low, Some(Reason::Level)));
    let mut spent_out = base.clone();
    spent_out.actors.attrs.magic[hero as usize] = 0;
    cases.push(("no magic points", spent_out, Some(Reason::Points)));
    {
        let mut bare = base.clone();
        let kinds: Vec<u16> = (0..8)
            .filter(|i| mask >> i & 1 != 0)
            .map(|i| bare.magic.tables.reagents[i])
            .collect();
        let held: Vec<Slot> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
            .filter(|&o| {
                kinds.contains(&bare.objects.kind(o)) && bare.objects.contained_in(o, hero)
            })
            .collect();
        for o in held {
            crate::objects::remove(&mut bare, o);
        }
        cases.push(("no reagents", bare, Some(Reason::Reagents)));
    }
    for (label, c, want) in cases {
        let m = menu_of(&c, &files, at).expect("a menu");
        let r = reason(&m, door, menu::CAST).expect("Cast shown");
        let mut e = prompt_engine(c.clone(), &files);
        choose(&mut e, menu::CAST, at);
        let did = points(&c, &e.state());
        eprintln!(
            "{label}: enabled {}, cast {}",
            u8::from(r.is_none()),
            u8::from(did)
        );
        assert_eq!(r.is_none(), did, "{label}");
        assert_eq!(r, want, "{label}");
    }
}

/// Spec 17 round 3, item 8, Cast aboard as a matched pair: one book-equipped state (the spell of
/// [`casts_checks_at_a_target_agree_with_cast`], its reagents, level and magic) on foot and the
/// same state aboard (A4-0x7b7a = the party size), each menu asked on that exact state, then Cast
/// run on it: on foot enabled and the spell is cast (a magic point spent, no aboard line); aboard
/// disabled as aboard and the loop's refusal (0x10498e: the aboard line, A4-0x744d) instead, no
/// magic spent.
#[test]
fn cast_with_the_same_book_runs_on_foot_and_is_refused_aboard() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let at = at_slot(&s, door);
    let hero = leader_slot(&s);
    let spell = [0x09u8, 0x07, 0x00, 0x05, 0x06, 0x03]
        .into_iter()
        .find(|&sp| s.magic.tables.mask(usize::from(sp)) != 0)
        .expect("a first-circle spell with reagents");
    let mut foot = s.clone();
    with_book(&mut foot, spell);
    let on_board = aboard(&foot);
    let aboard_line = super::text::a4_string(&s, -0x744d);
    let magic = move |st: &State| st.actors.attrs.magic[hero as usize];
    for (label, st, want) in [
        ("on foot", foot, None),
        ("aboard", on_board, Some(Reason::Aboard)),
    ] {
        let m = menu_of(&st, &files, at).expect("a menu");
        let r = reason(&m, door, menu::CAST).expect("Cast shown");
        let mut e = prompt_engine(st.clone(), &files);
        let before = lines_of(&e.state()).len();
        choose(&mut e, menu::CAST, at);
        let lines = lines_of(&e.state());
        let refused = said_line(
            &lines[before.saturating_sub(1).min(lines.len())..],
            &aboard_line,
        );
        let cast = magic(&e.state()) < magic(&st);
        eprintln!(
            "Cast {label}: enabled {}, cast {}, the aboard line {}",
            u8::from(r.is_none()),
            u8::from(cast),
            u8::from(refused)
        );
        assert_eq!(r, want, "Cast {label}: the menu");
        assert_eq!(cast, want.is_none(), "Cast {label}: the spell");
        assert_eq!(refused, want.is_some(), "Cast {label}: the aboard line");
    }
}

/// Spec 17's gate flag (`ui.input.in_wait`): set while the engine waits in the event wait 0x106bbc
/// (the prompt's key read), clear while it waits anywhere else between two ticks: a step from
/// the prompt waits out its walk outside the key read, and the prompt's read sets it again.
#[test]
fn the_wait_flag_is_set_only_while_the_engine_waits_in_the_event_wait() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let step = [
        (Key::Right, 1i32, 0i32),
        (Key::Left, -1, 0),
        (Key::Down, 0, 1),
        (Key::Up, 0, -1),
    ]
    .into_iter()
    .find(|&(_, dx, dy)| walkable(&s, i32::from(bx) + dx, i32::from(by) + dy))
    .expect("a free tile beside the leader");
    let mut e = prompt_engine(s, &files);
    assert!(e.state().ui.input.in_wait, "the prompt's key read");
    e.tick(&[InputEvent::Key(step.0)], 16);
    let mut outside = 0;
    for _ in 0..600 {
        if at_prompt(&e, &[]) {
            break;
        }
        if !e.state().ui.input.in_wait {
            outside += 1;
        }
        e.tick(&[], 16);
    }
    eprintln!("ticks waited outside the key read: {outside}");
    assert!(outside > 0, "the walk waits outside the event wait");
    assert!(
        at_prompt(&e, &[]) && e.state().ui.input.in_wait,
        "the prompt's read again"
    );
}

// ---- spec 17, round 2 -------------------------------------------------------------------------

/// [`prompt_engine`] that also hands back the engine's own state cell, for a test that changes
/// the world between two ticks (as a player's other input would).
fn shared_engine(s: State, files: &Files) -> (Engine, Rc<RefCell<State>>) {
    let cell: Rc<RefCell<Option<Rc<RefCell<State>>>>> = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&cell);
    let mut e = Engine::with_game(s, files.clone(), move |st, io| {
        *keep.borrow_mut() = Some(Rc::clone(&st));
        async move { crate::main_loop::commands::command_loop(&st, &io).await }
    });
    for _ in 0..400 {
        if at_prompt(&e, &[]) {
            let st = cell.borrow().clone().expect("the game started");
            return (e, st);
        }
        e.tick(&[], 16);
    }
    panic!("the prompt never came up");
}

/// `letter` then a left press at `at` (once the command waits for its target: the loop out of
/// its prompt read, A4-0x7a2c clear, and A4-0x7a32 set), and the menu's run of `code` on `at`,
/// on two engines from `s`, tick for tick, until both are back at the prompt (at most 600 ticks).
fn letter_and_menu(
    s: &State,
    files: &Files,
    letter: char,
    code: u16,
    at: (i16, i16),
) -> (Engine, Engine) {
    let mut by_letter = prompt_engine(s.clone(), files);
    let mut by_menu = prompt_engine(s.clone(), files);
    let (icon, run) = Run::start(&by_menu.state(), code, at).expect("a command with a target");
    let mut letter_in = vec![InputEvent::Key(Key::Char(letter))];
    let mut menu_in = icon.to_vec();
    let (mut letter_waits, mut run) = (true, Some(run));
    for _ in 0..600 {
        let waits = {
            let st = by_letter.state();
            st.main.reading == 0 && st.ui.input.target_pending == 1
        };
        if letter_waits && letter_in.is_empty() && waits {
            letter_in.extend(menu::left_press(at.0, at.1));
            letter_waits = false;
        }
        by_letter.tick(&letter_in, 16);
        letter_in.clear();
        page_step(&mut by_menu, &mut menu_in, &mut run);
        let done = |e: &Engine| at_prompt(e, &[]);
        if !letter_waits && run.is_none() && done(&by_letter) && done(&by_menu) {
            return (by_letter, by_menu);
        }
    }
    panic!("the two runs did not both come back to the prompt");
}

/// Spec 17 round 2, item 0b: a crate (0xba, 0xc0) whose frame is not 1 keeps Use enabled: its case
/// prints the not-usable line and searches it all the same (`usecmd/cases.rs`, 0xba / 0xc0); open
/// (frame 0), the loaf inside comes out (`world::search`). Chosen from the menu, Use leaves the
/// same world as `u` and a press, the end-of-use step paid in both.
#[test]
fn a_crate_not_of_frame_one_keeps_use_enabled_and_use_searches_it() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (lx, ly) = leader_at(&s);
    let (cx, cy) = beside_tile(&s, lx, ly).expect("a free tile by the leader");
    let z = u16::from(s.map.centre.z);
    for kind in [0xbau16, 0xc0] {
        let mut c = s.clone();
        let tf = kind;
        let chest = crate::objects::create_on_map(&mut c.objects, tf, 0, 0, cx, cy, z);
        let loaf = crate::objects::create_in(&mut c, 0x80, crate::objects::IN_CONTAINER, 1, chest);
        assert!(chest >= 0 && loaf >= 0);
        crate::map::view::update_view(&mut c);
        let at = at_slot(&c, chest);
        let m = menu_of(&c, &files, at).expect("a menu on it");
        assert_eq!(
            reason(&m, chest, menu::USE),
            Some(None),
            "{kind:#x}: Use enabled"
        );
        let (a, b) = letter_and_menu(&c, &files, 'u', menu::USE, at);
        let (a, b) = (a.state().clone(), b.state().clone());
        assert_eq!(
            a.objects.place(loaf),
            0,
            "{kind:#x}: the letter's Use searched it"
        );
        assert_eq!(
            b.objects.place(loaf),
            0,
            "{kind:#x}: the menu's Use searched it"
        );
        assert!(
            a.objects == b.objects && a.rng == b.rng,
            "{kind:#x}: the same world"
        );
        assert!(
            spent(&c, &a, 5) && spent(&c, &b, 5),
            "{kind:#x}: the end of Use in both"
        );
    }
    s.rng.seed(0);
}

/// Spec 17 round 2, item 4b: a key queued as the prompt comes back does not carry the held press
/// into its own command. Cast without a book, from the menu, refuses before it reads a target;
/// as its prompt returns the hero gets a book and `c` is queued: that Cast opens the book and
/// waits for its own target, and the menu's press never lands in it (no magic point spent).
#[test]
fn a_key_queued_as_the_prompt_returns_does_not_take_the_held_press() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    without_books(&mut s);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let at = at_slot(&s, door);
    let hero = leader_slot(&s);
    let (mut e, st) = shared_engine(s, &files);
    let (icon, run) = Run::start(&e.state(), menu::CAST, at).expect("Cast takes a target");
    let (mut queued, mut run) = (icon.to_vec(), Some(run));
    let (mut keyed, mut mp) = (false, 0);
    for _ in 0..600 {
        let back = e.state().main.reading == 1 && e.io().pending_events() == 0;
        if !keyed && back && queued.is_empty() && e.state().ui.input.target_command != menu::CAST {
            let mut w = st.borrow_mut();
            with_book(&mut w, 0x09);
            mp = w.actors.attrs.magic[hero as usize];
            drop(w);
            queued.push(InputEvent::Key(Key::Char('c')));
            keyed = true;
        }
        page_step(&mut e, &mut queued, &mut run);
        if keyed && run.is_none() {
            break;
        }
    }
    assert!(keyed, "the prompt came back");
    for _ in 0..200 {
        page_step(&mut e, &mut queued, &mut run);
    }
    let w = st.borrow();
    assert!(run.is_none(), "the run is over");
    assert_eq!(w.actors.attrs.magic[hero as usize], mp, "no spell cast");
    assert!(
        menu::target_read(&w, menu::CAST),
        "the key's Cast waits for its own target"
    );
}

/// Spec 17 rounds 2 and 3, items 4e, 11 and 18: a measurement, not a check: the time of one right
/// press's query on the door's cell and on the hero's inventory slot, the mean of 50, with the one
/// shared copy and with a copy per entry ([`menu::query_fresh`]), printed as a receipt line (in
/// microseconds); only that each press gives a menu is asserted.
#[test]
#[allow(clippy::disallowed_types)] // a test may read the clock
fn receipt_the_time_of_one_right_press() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let at = at_slot(&s, door);
    let mut panel = s.clone();
    show_inventory(&mut panel);
    let io = Io::new(files.clone());
    for (st, at) in [(&mut s, at), (&mut panel, list_cell(0))] {
        st.main.reading = 1;
        let t = std::time::Instant::now();
        for _ in 0..50 {
            assert!(menu::query(st, &io, at.0, at.1).is_some());
        }
        let shared = t.elapsed().as_micros() / 50;
        let t = std::time::Instant::now();
        for _ in 0..50 {
            assert!(menu::query_fresh(st, &io, at.0, at.1).is_some());
        }
        let fresh = t.elapsed().as_micros() / 50;
        eprintln!("receipt: one right press: {shared} us shared, {fresh} us a copy per entry");
    }
}

/// A menu as codes only (the user's game text, its names, left out): its kind, cell and underlined
/// command, and each section's object and kind with its entries' code, reason and theft mark.
fn shape(m: &Option<Menu>) -> String {
    let Some(m) = m else {
        return "none".into();
    };
    let sections: Vec<String> = m
        .sections
        .iter()
        .map(|x| {
            let entries: Vec<String> = x
                .entries
                .iter()
                .map(|e| {
                    let why = e.reason.map_or("-", Reason::code);
                    format!("{:#x}:{why}:{}", e.code, u8::from(e.theft))
                })
                .collect();
            format!("{} {:?} [{}]", x.object, x.kind, entries.join(" "))
        })
        .collect();
    format!(
        "{:?} {:?} {:?}: {}",
        m.kind,
        m.cell,
        m.underlined,
        sections.join("; ")
    )
}

/// Spec 17 rounds 3 and 5, items 11 and 6: the press's one shared copy (kept for speed: the
/// receipt above) gives every entry the answer a fresh copy of its own gives
/// ([`menu::query_fresh`]: each check on the press's state as it was), so no check's writes skew a
/// later one (Attack after Cast's book and reagent sums above all): on every view cell and every
/// panel cell, in states where each check has something to write: the start by a door; a book with
/// its reagents, a bow and arrows, loaves near and far (Cast and Attack each enabled somewhere);
/// the book without reagents, bare hands (Cast disabled for them somewhere); aboard; the inventory
/// shown (its panel cells give menus too). Every view cell gives a menu in every state. A mismatch
/// is reported as codes only ([`shape`]).
#[test]
fn the_shared_copy_gives_every_entry_the_answer_of_a_fresh_copy() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let io = Io::new(files.clone());
    let hero = leader_slot(&s);
    let mut loaves = s.clone();
    loaf_in_view(&mut loaves, &files, 3);
    loaf_beside(&mut loaves);
    let mut armed_book = loaves.clone();
    armed(&mut armed_book, 0x29);
    crate::objects::create_in(
        &mut armed_book,
        0x37,
        crate::objects::IN_INVENTORY,
        20,
        hero,
    );
    with_book(&mut armed_book, 0x09);
    let mut no_reagents = loaves;
    armed(&mut no_reagents, 0);
    with_book(&mut no_reagents, 0x09);
    let reagents: Vec<u16> = no_reagents.magic.tables.reagents.to_vec();
    let held: Vec<Slot> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&o| {
            reagents.contains(&no_reagents.objects.kind(o))
                && no_reagents.objects.contained_in(o, hero)
        })
        .collect();
    for o in held {
        crate::objects::remove(&mut no_reagents, o);
    }
    let on_board = aboard(&armed_book);
    let mut panel = armed_book.clone();
    show_inventory(&mut panel);
    // Every view cell of the build's view, at its centre; the last column and row lie past the
    // shown rect's edge (`geom::SHOWN`: the view less half a cell each side), so theirs is 3
    // pixels inside that edge, still in the same cell.
    let (_, _, x1, y1) = crate::geom::SHOWN;
    let at = |k: i16, origin: u16, edge: u16| (origin as i16 + k * 16 + 8).min(edge as i16 - 3);
    let view: Vec<(i16, i16)> = (0..VIEW_H as i16)
        .flat_map(|r| (0..VIEW_W as i16).map(move |c| (at(c, MAP_X, x1), at(r, MAP_Y, y1))))
        .collect();
    let panel_cells: Vec<(i16, i16)> = (0..12)
        .map(list_cell)
        .chain((0..8).map(equipment_cell))
        .collect();
    let enabled = |ms: &[Menu], code: u16, want: Option<Reason>| {
        ms.iter()
            .flat_map(|m| &m.sections)
            .flat_map(|x| &x.entries)
            .any(|e| e.code == code && e.reason == want)
    };
    for (label, st) in [
        ("by the door", s),
        ("a book, a bow and arrows", armed_book),
        ("no reagents, bare hands", no_reagents),
        ("aboard", on_board),
        ("the inventory shown", panel),
    ] {
        let mut prompt = st.clone();
        prompt.main.reading = 1;
        let mut menus: Vec<Menu> = Vec::new();
        let mut on_panel = 0;
        for (k, &(x, y)) in view.iter().chain(&panel_cells).enumerate() {
            let shared = menu::query(&prompt, &io, x, y);
            let fresh = menu::query_fresh(&prompt, &io, x, y);
            assert!(
                shared == fresh,
                "{label}: the cell at ({x}, {y}): shared {}, fresh {}",
                shape(&shared),
                shape(&fresh)
            );
            on_panel += usize::from(k >= view.len() && shared.is_some());
            menus.extend(shared);
        }
        let on_view = menus.len() - on_panel;
        eprintln!(
            "{label}: {on_view} view menus, {on_panel} panel menus, each the same on a fresh copy"
        );
        assert_eq!(on_view, view.len(), "{label}: a menu on every view cell");
        if label == "the inventory shown" {
            assert!(on_panel > 0, "{label}: menus on the panel too");
        }
        if label == "a book, a bow and arrows" {
            assert!(
                enabled(&menus, menu::CAST, None),
                "{label}: Cast enabled somewhere"
            );
            assert!(
                enabled(&menus, menu::ATTACK, None),
                "{label}: Attack enabled somewhere"
            );
        }
        if label == "no reagents, bare hands" {
            let short = enabled(&menus, menu::CAST, Some(Reason::Reagents));
            assert!(short, "{label}: Cast disabled for its reagents somewhere");
        }
    }
}

/// Follow-up F1 (rounds 1 and 2), item 3 and its item 1: the fresh-copy switch through
/// [`menu::query_fresh`] itself, on a real press (the closed door's cell by the leader): the press
/// runs its checks on fresh copies (`menu::fresh_copies` goes up), a plain `query` on the same
/// cell runs none (it does not move); clear after a `query_fresh` that gave a menu (the same menu
/// `query` gives); a `query_fresh` inside an outer fresh scope leaves the switch set for the rest
/// of that scope, and it is clear again after it.
#[test]
fn query_fresh_puts_the_fresh_copy_switch_back() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let at = at_slot(&s, door);
    let io = Io::new(files.clone());
    let mut prompt = s.clone();
    prompt.main.reading = 1;
    assert!(!menu::fresh_now(), "clear before");
    let copies = menu::fresh_copies();
    let fresh = menu::query_fresh(&prompt, &io, at.0, at.1);
    let by_fresh = menu::fresh_copies() - copies;
    assert!(fresh.is_some(), "a menu");
    assert!(!menu::fresh_now(), "clear after query_fresh");
    let copies = menu::fresh_copies();
    let shared = menu::query(&prompt, &io, at.0, at.1);
    let by_shared = menu::fresh_copies() - copies;
    eprintln!("checks on a fresh copy: query_fresh {by_fresh}, query {by_shared}");
    assert!(by_fresh > 0, "query_fresh runs its checks on fresh copies");
    assert_eq!(by_shared, 0, "query runs none on a fresh copy");
    assert!(fresh == shared, "the menu query gives");
    let (inner, set) = menu::fresh_scope(|| {
        let m = menu::query_fresh(&prompt, &io, at.0, at.1);
        (m, menu::fresh_now())
    });
    assert!(
        set,
        "an outer scope's switch still set after the inner query_fresh"
    );
    assert!(inner == fresh, "the same menu inside the outer scope");
    assert!(!menu::fresh_now(), "clear after the outer scope");
}

/// A loaf (0x80) made on a free tile next to the leader, the view rebuilt; its slot and pixel.
fn loaf_beside(s: &mut State) -> (Slot, (i16, i16)) {
    let (lx, ly) = leader_at(s);
    let (x, y) = beside_tile(s, lx, ly).expect("a free tile by the leader");
    let z = u16::from(s.map.centre.z);
    let loaf = crate::objects::create_on_map(&mut s.objects, 0x80, 0, 1, x, y, z);
    assert!(loaf >= 0);
    crate::map::view::update_view(s);
    (loaf, at_slot(s, loaf))
}

/// A loaf made on a free tile `d` cells from the leader (on the farther axis) that the view
/// shows (its menu names it), the view rebuilt; its slot and pixel.
fn loaf_in_view(s: &mut State, files: &Files, d: i32) -> (Slot, (i16, i16)) {
    let (lx, ly) = leader_at(s);
    let z = u16::from(s.map.centre.z);
    let ring = (-d..=d).flat_map(|a| [(a, -d), (a, d), (-d, a), (d, a)]);
    for (dx, dy) in ring {
        let (x, y) = (i32::from(lx) + dx, i32::from(ly) + dy);
        if !walkable(s, x, y) {
            continue;
        }
        let mut c = s.clone();
        let loaf = crate::objects::create_on_map(&mut c.objects, 0x80, 0, 1, x as u16, y as u16, z);
        crate::map::view::update_view(&mut c);
        let at = at_slot(&c, loaf);
        if menu_of(&c, files, at).is_some_and(|m| m.sections.iter().any(|x| x.object == loaf)) {
            *s = c;
            return (loaf, at);
        }
    }
    panic!("no tile {d} cells off in view");
}

/// Spec 17 round 2, item 4: a button-less pointer event queued before the held target press (a
/// release, at another cell) lets the press go in (the queues stay quiet), selects nothing on its
/// own (0x106bbc with no button down gives no code) and walks no one: the press then takes the
/// loaf.
#[test]
fn a_queued_button_release_before_the_target_press_selects_nothing() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let (loaf, at) = loaf_beside(&mut s);
    let elsewhere = at_slot(&s, door);
    let mut e = prompt_engine(s.clone(), &files);
    let (icon, run) = Run::start(&e.state(), menu::GET, at).expect("Get takes a target");
    let (mut queued, mut run) = (icon.to_vec(), Some(run));
    let mut released = 0;
    for _ in 0..600 {
        let reads = run.is_some() && menu::target_read(&e.state(), menu::GET);
        if reads && queued.is_empty() && e.io().pending_events() == 0 {
            let (x, y) = elsewhere;
            queued.push(InputEvent::Pointer { x, y, buttons: 0 });
            released += 1;
        }
        page_step(&mut e, &mut queued, &mut run);
        if run.is_none() && at_prompt(&e, &queued) {
            break;
        }
    }
    eprintln!("releases queued before the press: {released}");
    assert_eq!(
        released, 1,
        "the release went in first, the press in the same step"
    );
    let after = e.state();
    assert_eq!(leader_at(&after), (bx, by), "no walk");
    assert!(!still_there(&s, &after, loaf), "the press took the loaf");
}

/// The hero (leader) with only `weapon` readied (its type), the readied things before taken
/// away; 0 for bare hands.
fn armed(s: &mut State, weapon: u16) -> Slot {
    let hero = leader_slot(s);
    let readied: Vec<Slot> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&o| s.objects.place(o) == crate::objects::READIED && s.objects.parent(o) == hero)
        .collect();
    for o in readied {
        crate::objects::remove(s, o);
    }
    if weapon == 0 {
        return 0;
    }
    let w = crate::objects::create_in(s, weapon, crate::objects::READIED, 1, hero);
    assert!(w >= 0);
    w
}

/// The arrows (0x37) `hero` carries.
fn arrows(s: &State, hero: Slot) -> u16 {
    (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&o| s.objects.kind(o) == 0x37 && s.objects.contained_in(o, hero))
        .map(|o| s.objects.qq_of(o) & 0xff)
        .sum()
}

/// The format addresses of the lines combat said after `before`, only those: its queue keeps the
/// last ones (`combat.said`, the oldest dropped first), so `after` is a tail of `before` followed
/// by the new ones; the longest such tail is taken as the kept part. The limit: once lines were
/// dropped (the queue at its cap), new lines that repeat what it held can be taken for its kept
/// part, and count as fewer ([`said_since_misses_only_a_full_queues_repeats`]); the Attack checks
/// start from an empty queue, which one command never fills, so they are exact.
fn said_since(before: &State, after: &State) -> Vec<u32> {
    let (old, now) = (&before.combat.said, &after.combat.said);
    let kept = (0..=old.len())
        .map(|dropped| &old[dropped..])
        .find(|tail| now.starts_with(tail))
        .map_or(0, <[_]>::len);
    now[kept..].iter().map(|x| x.at).collect()
}

/// The queue of 0x108658's lines (`combat.said`) after the engine says `ats` in order, through
/// its own [`crate::combat::present::say`] (the text on, which it needs; outside a poll there is
/// no `Io`, so nothing is printed): its cap and its dropping are the engine's.
fn said_queue(ats: &[u32]) -> State {
    let mut s = State::default();
    s.ui.text.on = true;
    for &at in ats {
        crate::combat::present::say(&mut s, at, Vec::new());
    }
    s
}

/// The said queue's cap, as the engine keeps it: what is left after more lines than it holds.
fn said_cap() -> usize {
    said_queue(&[0; 200]).combat.said.len()
}

/// Follow-up F1 (rounds 1 and 2), items 2 and 3: [`said_since`] on the engine's own keeping of the
/// queue (its cap read from it, the oldest dropped first): exact while nothing was dropped, and
/// after drops when the new lines differ from what it held; its limit, pinned: once lines were
/// dropped, new lines that repeat what it held count as fewer: a full queue of one line, that
/// line again gives none (the queue after is the queue before), that line and another give only
/// the other.
#[test]
fn said_since_misses_only_a_full_queues_repeats() {
    let cap = said_cap();
    assert!(cap > 4 && cap < 200, "a cap");
    let (a, b, c) = (1u32, 2, 3);
    let some = said_queue(&[a, b]);
    let more: Vec<u32> = [a, b, c, a].to_vec();
    assert_eq!(
        said_since(&some, &said_queue(&more)),
        [c, a],
        "not full: exact"
    );
    assert_eq!(said_since(&some, &some), Vec::<u32>::new(), "nothing said");
    let lines: Vec<u32> = (0..cap as u32).map(|k| 100 + k).collect();
    let full = said_queue(&lines);
    assert_eq!(full.combat.said.len(), cap, "at its cap");
    let then: Vec<u32> = lines.iter().copied().chain([a, b]).collect();
    assert_eq!(
        said_since(&full, &said_queue(&then)),
        [a, b],
        "full, new lines: exact"
    );
    let ones = vec![a; cap];
    let same = said_queue(&ones);
    let again = said_queue(&[ones.as_slice(), &[a]].concat());
    assert!(
        again.combat.said == same.combat.said,
        "one more of the same: the queue as it was"
    );
    assert_eq!(
        said_since(&same, &again),
        Vec::<u32>::new(),
        "the limit: none"
    );
    let and_b = said_queue(&[ones.as_slice(), &[a, b]].concat());
    assert_eq!(
        said_since(&same, &and_b),
        [b],
        "the limit: a, b said, b counted"
    );
}

/// Spec 17 rounds 2, 3 and 5, items 4c, 14 and 9, Attack (0x124caa, 0x124a98, 0x124232), judged on
/// the copy over the weapons 0x1249de lists, against the real Attack run from the menu, both ways,
/// each refusal at its own checkpoint, the line the handler says (combat's said queue, by its
/// format address, the entries added by the run): a bow (0x29) without arrows (ammo: A4-0x76dc
/// said, nothing flies); the bow with arrows (enabled: an arrow flies, 0x124232's missile takes
/// it); bare hands at a loaf three cells off (far: A4-0x7512 said, the attack target forgotten);
/// bare hands at a loaf next to the leader (enabled: the run gets past 0x124232's range check, the
/// leader turned away beforehand now faces the loaf, the attack target kept, no refusal said; the
/// hit itself is not pinned: A4+0x185c, `combat.hit`, is rewritten by the turns after it); the bow
/// and arrows with a foe next to the fighting leader (blocked: A4-0x75f2 said, nothing flies).
#[test]
fn attacks_ammo_and_range_checks_agree_with_attack() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let (far_loaf, at_far) = loaf_in_view(&mut s, &files, 3);
    let (near_loaf, at_near) = loaf_beside(&mut s);
    let hero = leader_slot(&s);
    let (lx0, ly0) = leader_at(&s);
    let a4 = crate::image::A4;
    let (no_arrows, out_of_range, interferes) = (a4 - 0x76dc, a4 - 0x7512, a4 - 0x75f2);
    let refusals = [no_arrows, out_of_range, interferes];
    let cap = said_cap();
    type Checkpoint = Box<dyn Fn(&State, &State) -> bool>;
    // A case: its label, its state, the loaf aimed at and its pixel, the menu's answer, the check.
    type Case = (
        &'static str,
        State,
        (Slot, (i16, i16)),
        Option<Reason>,
        Checkpoint,
    );
    let flew: Checkpoint = Box::new(move |b, a| arrows(a, hero) < arrows(b, hero));
    // 0x124232 past its range check turns the attacker toward the target; the leader is turned
    // away first, so facing the loaf afterwards (and the attack target kept) pins that the run got
    // past the range check, onto the hit's path (the hit itself is not pinned).
    let (nx, ny) = {
        let p = s.objects.location[near_loaf as usize];
        (p.x as i16 - lx0 as i16, p.y as i16 - ly0 as i16)
    };
    let toward: u8 = if nx.abs() > ny.abs() {
        if nx < 0 { 6 } else { 2 }
    } else if ny < 0 {
        0
    } else {
        4
    };
    let faces = move |a: &State| a.actors.attrs.movement_flags[hero as usize] & 7 == toward;
    let landed: Checkpoint = Box::new(move |_, a| faces(a) && a.main.attack_target == near_loaf);
    let said = |at: u32| -> Checkpoint {
        Box::new(move |b, a| said_since(b, a).contains(&at) && arrows(a, hero) == arrows(b, hero))
    };
    let mut bow = s.clone();
    armed(&mut bow, 0x29);
    let bare_bow = bow.clone();
    crate::objects::create_in(&mut bow, 0x37, crate::objects::IN_INVENTORY, 20, hero);
    let mut hands = s.clone();
    armed(&mut hands, 0);
    let mf = &mut hands.actors.attrs.movement_flags[hero as usize];
    *mf = (*mf & 0xf8) | (toward ^ 4);
    let mut blocked = bow.clone();
    let (lx, ly) = leader_at(&blocked);
    let z = u16::from(blocked.map.centre.z);
    let foe = (1..FIRST_TEMP_ACTOR)
        .find(|&a| {
            let o = &blocked.objects;
            a != hero
                && o.type_frame[a as usize] != 0
                && o.place(a) == 0
                && blocked.actors.attrs.status[a as usize] & 0x96 == 0
                && blocked.actors.attrs.movement_flags[a as usize] & 0x10 == 0
        })
        .expect("an actor to stand in the way");
    let (fx, fy) = beside_tile(&blocked, lx, ly).expect("a free tile by the leader");
    crate::objects::move_to(&mut blocked, foe, fx, fy, z);
    blocked.actors.worktype[hero as usize] = 3;
    blocked.actors.attrs.unknown_18f1[hero as usize] = foe as u8;
    crate::map::view::update_view(&mut blocked);
    let cases: Vec<Case> = vec![
        (
            "a bow, no arrows",
            bare_bow,
            (far_loaf, at_far),
            Some(Reason::Ammo),
            said(no_arrows),
        ),
        ("a bow and arrows", bow, (far_loaf, at_far), None, flew),
        (
            "bare hands, three cells off",
            hands.clone(),
            (far_loaf, at_far),
            Some(Reason::Far),
            said(out_of_range),
        ),
        (
            "bare hands, next to it",
            hands,
            (near_loaf, at_near),
            None,
            landed,
        ),
        (
            "a foe in the way",
            blocked,
            (far_loaf, at_far),
            Some(Reason::Blocked),
            said(interferes),
        ),
    ];
    for (label, mut c, (target, at), want, checkpoint) in cases {
        c.main.attack_target = -1;
        c.combat.hit = -1;
        // An empty said queue: one command never fills it, so `said_since` is exact here.
        c.combat.said.clear();
        let m = menu_of(&c, &files, at).expect("a menu");
        let r = reason(&m, target, menu::ATTACK).expect("Attack shown");
        let mut e = prompt_engine(c.clone(), &files);
        assert!(choose(&mut e, menu::ATTACK, at), "{label}: its target read");
        let after = e.state().clone();
        assert!(
            after.combat.said.len() < cap,
            "{label}: the said queue never full"
        );
        let reached = checkpoint(&c, &after);
        let refused = said_since(&c, &after).iter().any(|x| refusals.contains(x));
        eprintln!(
            "{label}: enabled {}, checkpoint {}, a refusal said {}",
            u8::from(r.is_none()),
            u8::from(reached),
            u8::from(refused)
        );
        assert_eq!(r, want, "{label}: the menu");
        assert!(reached, "{label}: the handler's checkpoint");
        assert_eq!(
            refused,
            r.is_some(),
            "{label}: a refusal said exactly when disabled"
        );
    }
}

/// The hero's inventory as `show_inventory` lays it out after `make` adds to it; the list cell
/// or equipment cell pixel of `obj` there.
fn cell_of_carried(s: &mut State, obj: Slot) -> (i16, i16) {
    let listed = show_inventory(s);
    if let Some(k) = listed.iter().take(12).position(|&o| o == obj) {
        return list_cell(k);
    }
    let k = (0..8)
        .find(|&k| s.inventory.equip(k) == obj)
        .expect("the object on the panel");
    equipment_cell(k)
}

/// The hero's readied things taken away, the inventory panel shown.
fn bare_hero(s: &mut State) -> Slot {
    let hero = Slot::from(s.actors.party.members[0]);
    let readied: Vec<Slot> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&o| s.objects.place(o) == crate::objects::READIED && s.objects.parent(o) == hero)
        .collect();
    for o in readied {
        crate::objects::remove(s, o);
    }
    hero
}

/// Spec 17 Acceptance 4 (round 2, item 9), on made objects: a loaf the hero carries (usable:
/// eaten), a bag (a container of the set A4-0x670c), a shovel readied (0x67). Each gives one
/// section, its own action first, then Use, Look, Drop, Move, exactly: the loaf readied (0xa7),
/// the bag opened (0xa0), the shovel taken off (0xa8); Use only where the type is in the use list.
#[test]
fn an_inventory_object_gives_the_panel_menu_with_its_own_action_first() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let hero = bare_hero(&mut s);
    let made = |s: &mut State, kind: u16, place: u8| {
        let o = crate::objects::create_in(s, kind, place, 1, hero);
        assert!(o >= 0);
        o
    };
    let loaf = made(&mut s, 0x80, crate::objects::IN_INVENTORY);
    let bag_kind = bag_type(&s);
    let bag = made(&mut s, bag_kind, crate::objects::IN_INVENTORY);
    let shovel = made(&mut s, 0x67, crate::objects::READIED);
    for (label, obj, own) in [
        ("a loaf", loaf, menu::READY),
        ("a bag", bag, menu::OPEN),
        ("a readied shovel", shovel, menu::UNREADY),
    ] {
        let mut c = s.clone();
        let at = cell_of_carried(&mut c, obj);
        let m = menu_of(&c, &files, at).expect("a panel menu");
        assert!(
            m.kind == menu::Kind::Panel && m.cell.is_none(),
            "{label}: panel"
        );
        assert_eq!(m.sections.len(), 1, "{label}: one section");
        assert_eq!(m.sections[0].object, obj, "{label}: its own");
        let usable = super::in_type_list(&c, c.objects.kind(obj));
        let mut want = vec![own];
        if usable {
            want.push(menu::USE);
        }
        want.extend([menu::LOOK, menu::DROP, menu::MOVE]);
        eprintln!("{label}: entries {}, use {}", want.len(), u8::from(usable));
        assert_eq!(codes(&m, obj), want, "{label}: the entries");
        let all_enabled = m.sections[0].entries.iter().all(|e| e.enabled());
        assert!(all_enabled, "{label}: each can run");
    }
    assert!(super::in_type_list(&s, 0x80), "the loaf is usable");
    let empty = (0..12).find(|&k| {
        let mut c = s.clone();
        show_inventory(&mut c);
        c.inventory.cells[k] == 0
    });
    if let Some(k) = empty {
        let mut c = s.clone();
        show_inventory(&mut c);
        assert!(
            menu_of(&c, &files, list_cell(k)).is_none(),
            "an empty cell: none"
        );
    }
}

/// Spec 17 rounds 2 to 4, items 4c, 9 and 10, the panel's own checks, each against the real
/// handler run from the menu (forced where the menu hides the entry). A type 0x4c, readied or not,
/// shows neither Drop nor Move: the object move refuses that type every time (0x10c9d2), the
/// kind's own condition. Run anyway, Drop of the readied amulet of submission stays (0x1122dc: the
/// panel only redrawn; a drop destination supplied, still readied, no movement points), and of
/// the same amulet carried, not readied, it runs its step (the drop's 3 movement points) and the
/// amulet stays carried all the same. The readied one's own entry, the take-off, is disabled as
/// staying and the left press leaves it readied. Otherwise the same state, a readied shovel: Drop
/// is enabled and puts it down on the destination, its take-off is enabled and the left press
/// takes it off. The Open of a chest of frame bit 1 is locked (0x10619c's locked line, the chest
/// still closed); Move of what another member carries while that member is out of view
/// (0x112bf2) is refused.
#[test]
fn the_panels_stays_locked_and_away_checks_agree_with_the_handlers() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let hero = bare_hero(&mut s);
    let (lx, ly) = leader_at(&s);
    let (tx, ty) = beside_tile(&s, lx, ly).expect("a free tile by the leader");
    let free = cell_pixel(&s, tx, ty);
    let spot = menu::left_press(free.0, free.1);
    let left = |e: &mut Engine, at: (i16, i16)| {
        let [press, release] = menu::left_press(at.0, at.1);
        e.tick(&[press, release], 16);
        for _ in 0..200 {
            e.tick(&[], 16);
        }
    };
    let unworn = s.clone();
    let amulet = crate::objects::create_in(&mut s, 0x4c, crate::objects::READIED, 1, hero);
    let mut c = s.clone();
    let at = cell_of_carried(&mut c, amulet);
    let m = menu_of(&c, &files, at).expect("a panel menu");
    assert_eq!(reason(&m, amulet, menu::DROP), None, "Drop hidden on 0x4c");
    assert_eq!(
        reason(&m, amulet, menu::UNREADY),
        Some(Some(Reason::Stays)),
        "the take-off stays"
    );
    assert_eq!(reason(&m, amulet, menu::MOVE), None, "Move hidden on 0x4c");
    let mut e = prompt_engine(c.clone(), &files);
    choose_then(&mut e, menu::DROP, at, &[&spot]);
    let (place, ran) = (e.state().objects.place(amulet), spent(&c, &e.state(), 3));
    eprintln!(
        "the readied amulet: Drop put down 0, its step run {}",
        u8::from(ran)
    );
    assert_eq!(place, crate::objects::READIED, "Drop: it stays");
    assert!(!ran, "Drop: refused before its step (no 3 points)");
    let mut e = prompt_engine(c.clone(), &files);
    left(&mut e, at);
    assert_eq!(
        e.state().objects.place(amulet),
        crate::objects::READIED,
        "the left press: it stays"
    );
    // The stays predicate's other branches, otherwise the same state and the same destination:
    // the amulet carried, not readied (Drop hidden, forced), and a readied shovel in its place
    // (Drop enabled). Drop runs its step (the drop's 3 movement points, 0x1285fe's put-down) on
    // both; the shovel lands on the tile, the amulet stays carried, as 0x10c9d2 refuses to move a
    // 0x4c anywhere.
    for (label, kind, place) in [
        ("the amulet carried", 0x4c, crate::objects::IN_INVENTORY),
        ("a readied shovel", 0x67, crate::objects::READIED),
    ] {
        let mut st = unworn.clone();
        let obj = crate::objects::create_in(&mut st, kind, place, 1, hero);
        let at = cell_of_carried(&mut st, obj);
        let m = menu_of(&st, &files, at).expect("a panel menu");
        let want = (kind != 0x4c).then_some(None);
        assert_eq!(reason(&m, obj, menu::DROP), want, "{label}: Drop");
        let mut e = prompt_engine(st.clone(), &files);
        choose_then(&mut e, menu::DROP, at, &[&spot]);
        let after = e.state();
        let ran = spent(&st, &after, 3);
        let put = after.objects.place(obj) == 0
            && (after.objects.x(obj), after.objects.y(obj)) == (tx, ty);
        eprintln!(
            "{label}: Drop shown {}, its step run {}, put down {}",
            u8::from(want.is_some()),
            u8::from(ran),
            u8::from(put)
        );
        assert!(ran, "{label}: Drop reaches its step");
        assert_eq!(put, kind != 0x4c, "{label}: on the destination");
    }
    let mut worn = unworn.clone();
    let shovel = crate::objects::create_in(&mut worn, 0x67, crate::objects::READIED, 1, hero);
    let at_shovel = cell_of_carried(&mut worn, shovel);
    let m = menu_of(&worn, &files, at_shovel).expect("a panel menu");
    assert_eq!(
        reason(&m, shovel, menu::UNREADY),
        Some(None),
        "a shovel: the take-off runs"
    );
    let mut e = prompt_engine(worn.clone(), &files);
    left(&mut e, at_shovel);
    let off = e.state().objects.place(shovel) == crate::objects::IN_INVENTORY;
    eprintln!("a readied shovel: taken off {}", u8::from(off));
    assert!(off, "a shovel: the left press takes it off");

    let mut locked = s.clone();
    let chest = crate::objects::create_in(
        &mut locked,
        0x62 | 3 << 10,
        crate::objects::IN_INVENTORY,
        0,
        hero,
    );
    let at = cell_of_carried(&mut locked, chest);
    let m = menu_of(&locked, &files, at).expect("a panel menu");
    assert_eq!(codes(&m, chest).first(), Some(&menu::OPEN));
    assert_eq!(
        reason(&m, chest, menu::OPEN),
        Some(Some(Reason::Locked)),
        "locked"
    );
    let mut open = locked.clone();
    open.objects.type_frame[chest as usize] = 0x62 | 1 << 10;
    let m = menu_of(&open, &files, at).expect("a panel menu");
    assert_eq!(
        reason(&m, chest, menu::OPEN),
        Some(None),
        "closed, not locked: Open runs"
    );
    for (label, st, opens) in [("locked", &locked, false), ("closed", &open, true)] {
        let mut e = prompt_engine(st.clone(), &files);
        let [press, release] = menu::left_press(at.0, at.1);
        e.tick(&[press, release], 16);
        for _ in 0..200 {
            e.tick(&[], 16);
        }
        let shown = e.state().ui.panel.container as Slot;
        eprintln!("{label}: opened {}", u8::from(shown == chest));
        assert_eq!(shown == chest, opens, "{label}: the left press");
    }

    let mut away = s.clone();
    let other = Slot::from(away.actors.party.members[1]);
    // A shovel (0x67): carried, not counted (no stack to merge into).
    let gem = crate::objects::create_in(&mut away, 0x67, crate::objects::IN_INVENTORY, 1, other);
    let (ox, oy) = (away.objects.x(other), away.objects.y(other));
    let z = u16::from(away.map.centre.z);
    for (label, far) in [("in view", false), ("out of view", true)] {
        let mut c = away.clone();
        if far {
            crate::objects::move_to(&mut c, other, ox + 40, oy, z);
        }
        c.ui.panel.mode = 0x92;
        c.ui.panel.member = 1;
        c.ui.panel.container = other as u16;
        crate::inventory::recount(&mut c, other);
        let listed = listed_children(&mut c, other);
        for (k, cell) in c.inventory.cells[..12].iter_mut().enumerate() {
            *cell = listed.get(k).copied().unwrap_or(0);
        }
        let k = listed.iter().position(|&o| o == gem).expect("listed");
        let m = menu_of(&c, &files, list_cell(k)).expect("a panel menu");
        let r = reason(&m, gem, menu::MOVE).expect("Move shown");
        let mut e = prompt_engine(c.clone(), &files);
        let hero_cell = cell_pixel(&c, c.objects.x(hero), c.objects.y(hero));
        let to_hero = [
            InputEvent::Pointer {
                x: hero_cell.0,
                y: hero_cell.1,
                buttons: 1,
            },
            InputEvent::Pointer {
                x: hero_cell.0,
                y: hero_cell.1,
                buttons: 0,
            },
        ];
        choose_then(&mut e, menu::MOVE, list_cell(k), &[&to_hero]);
        let moved = !carried_by(&e.state(), other, gem);
        eprintln!(
            "{label}: enabled {}, moved {}",
            u8::from(r.is_none()),
            u8::from(moved)
        );
        assert_eq!(r, far.then_some(Reason::Screen), "{label}");
        assert_eq!(r.is_none(), moved, "{label}");
    }
}

/// Spec 17 rounds 4 and 5, items 9 and 4: no menu offers Drop or Move on a type 0x4c, worn or not,
/// nor Ready on an unworn one: the object moves refuse that type at the prompt (0x10c9d2 always,
/// 0x10cda8 unless the game allows it, `objects::moves::FIXED_TYPE`). The hero carrying one
/// unworn and, in another state, one readied, each the hero's inventory panel shown: the section
/// of the 0x4c lists literally Look alone (unworn) or the take-off and Look (readied; the take-off
/// disabled as staying); no Use (the type is outside Use's list), no Drop, no Move. The left press
/// the hidden Ready would give, made anyway, leaves the unworn one carried, not readied, while the
/// same press on the same list cell readies an unworn shovel there (the matched control, follow-up
/// F1 item 1). The map's menu of the leader's cell never names it.
#[test]
fn a_type_0x4c_worn_or_not_is_never_offered_drop() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let hero = bare_hero(&mut s);
    for (label, place, want) in [
        ("unworn", crate::objects::IN_INVENTORY, vec![menu::LOOK]),
        (
            "readied",
            crate::objects::READIED,
            vec![menu::UNREADY, menu::LOOK],
        ),
    ] {
        let mut st = s.clone();
        let amulet = crate::objects::create_in(&mut st, 0x4c, place, 1, hero);
        assert!(amulet >= 0);
        let at = cell_of_carried(&mut st, amulet);
        let m = menu_of(&st, &files, at).expect("a panel menu");
        let got = codes(&m, amulet);
        eprintln!("{label}: entries {}", got.len());
        assert!(
            !super::in_type_list(&st, 0x4c),
            "0x4c is outside Use's list"
        );
        assert_eq!(
            got, want,
            "{label}: literally these, no Ready, Drop or Move"
        );
        if place == crate::objects::IN_INVENTORY {
            let pressed = |st: &State| {
                let mut e = prompt_engine(st.clone(), &files);
                let [press, release] = menu::left_press(at.0, at.1);
                e.tick(&[press, release], 16);
                for _ in 0..200 {
                    e.tick(&[], 16);
                }
                e
            };
            let kept = pressed(&st).state().objects.place(amulet) == crate::objects::IN_INVENTORY;
            // The matched control: the same press on the same list cell, an unworn shovel there
            // instead, readies it, so the press reaches the slot's own action (0x10619c's Ready)
            // and the amulet staying carried is that action refusing it, not a press that
            // reached nothing.
            let mut control = s.clone();
            let shovel = crate::objects::create_in(&mut control, 0x67, place, 1, hero);
            assert_eq!(
                cell_of_carried(&mut control, shovel),
                at,
                "the same list cell"
            );
            let readied =
                pressed(&control).state().objects.place(shovel) == crate::objects::READIED;
            eprintln!(
                "{label}: the left press anyway, still carried {}; on a shovel there, readied {}",
                u8::from(kept),
                u8::from(readied)
            );
            assert!(
                readied,
                "the control: the same press readies an unworn shovel"
            );
            assert!(
                kept,
                "{label}: the left press leaves it carried, not readied"
            );
        }
        let (lx, ly) = leader_at(&st);
        let lead = menu_of(&st, &files, cell_pixel(&st, lx, ly));
        let named = lead.is_some_and(|m| m.sections.iter().any(|x| x.object == amulet));
        assert!(!named, "{label}: the map's menu never names it");
    }
}

/// Spec 17 Acceptance 3 (rounds 2 to 4, items 8 and 6): an actor on a chair, built for the test (a
/// townsperson in view moved onto a made chair, 0xfc, next to the leader): two sections, the
/// actor's then the chair's, with literally these commands: the actor Talk, Look, Move, Cast (the
/// cell's), Attack; the chair Get alone. Use is hidden on the chair by spec 17's tier rule: a
/// command whose kind test fails is hidden (tier K), and Use's kind test, its 85 types, does not
/// hold the chair's type.
#[test]
fn an_actor_on_a_chair_gives_the_actors_section_then_the_chairs() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let (lx, ly) = leader_at(&s);
    let (cx, cy) = beside_tile(&s, lx, ly).expect("a free tile by the leader");
    let z = u16::from(s.map.centre.z);
    let chair = crate::objects::create_on_map(&mut s.objects, 0xfc, 0, 0, cx, cy, z);
    let npc = (1..FIRST_TEMP_ACTOR)
        .find(|&a| {
            let o = &s.objects;
            o.type_frame[a as usize] != 0
                && o.place(a) == 0
                && s.actors.attrs.status[a as usize] & 0x96 == 0
                && crate::converse::start::refusal(&s, leader_slot(&s), a).is_none()
                && crate::converse::start::may_have_script(a, o.kind(a))
        })
        .expect("a townsperson who would talk");
    crate::objects::move_to(&mut s, npc, cx, cy, z);
    crate::map::view::update_view(&mut s);
    let m = menu_of(&s, &files, at_slot(&s, npc)).expect("a menu");
    let objects: Vec<Slot> = m.sections.iter().map(|x| x.object).collect();
    assert_eq!(
        objects,
        vec![npc, chair],
        "the actor's section, then the chair's"
    );
    let mut actor = codes(&m, npc);
    actor.sort_unstable();
    let mut want = vec![menu::TALK, menu::LOOK, menu::MOVE, menu::CAST, menu::ATTACK];
    want.sort_unstable();
    assert_eq!(actor, want, "the actor's commands");
    // Spec 17's tier rule: a command whose kind test fails is hidden (tier K). Use's kind test
    // is its type list (the 85 types of A4-0x66dc, 0x128b6a), which does not hold the chair's
    // type, so the chair offers Get alone.
    assert!(
        !super::in_type_list(&s, 0xfc),
        "the chair's type is outside Use's 85 types: Use's kind test fails (spec 17 tier K)"
    );
    assert_eq!(
        codes(&m, chair),
        vec![menu::GET],
        "the chair's commands: Get alone; Use hidden by spec 17's tier rule (tier K: its kind test, the 85 types, fails)"
    );
}

/// Spec 17 round 2, item 7: the engine names the underlined command (A4-0x7b09) in every menu,
/// Rest and Combat included; none when nothing is underlined.
#[test]
fn the_menu_names_the_underlined_command() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (door, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let at = at_slot(&s, door);
    crate::ui::globals::set_last_command(&mut s.actors, 0xff);
    assert_eq!(menu_of(&s, &files, at).expect("a menu").underlined, None);
    for code in [menu::LOOK, menu::REST, menu::COMBAT] {
        crate::ui::globals::set_last_command(&mut s.actors, code as u8);
        let m = menu_of(&s, &files, at).expect("a menu");
        assert_eq!(m.underlined, Some(code), "{code:#x}");
    }
}

/// Spec 17 Acceptance 7 (Wide, round 2, item 11): Look is shown and enabled on any cell of the
/// view; every other entry of an object 6 cells from the leader (one past the original's
/// 5-cell window, literal) is disabled as window, and one 5 cells off is not: Get there says
/// far. Run, Get's press 6 cells off gives the command nothing (it still waits for its target);
/// 5 cells off Get reads it and refuses (back at the prompt).
#[cfg(feature = "wide")]
#[test]
fn wide_outside_the_window_every_entry_but_look_is_disabled_as_window() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    for (d, want) in [(5, Reason::Far), (6, Reason::Window)] {
        let mut c = s.clone();
        let (loaf, at) = loaf_in_view(&mut c, &files, d);
        let m = menu_of(&c, &files, at).expect("a menu");
        let section = m
            .sections
            .iter()
            .find(|x| x.object == loaf)
            .expect("the loaf's");
        assert_eq!(
            reason(&m, loaf, menu::LOOK),
            Some(None),
            "{d}: Look enabled"
        );
        assert_eq!(reason(&m, loaf, menu::GET), Some(Some(want)), "{d}: Get");
        let others = section
            .entries
            .iter()
            .filter(|e| e.code != menu::LOOK)
            .count();
        assert!(others > 0, "{d}: a named entry besides Look");
        if d == 6 {
            assert!(
                section
                    .entries
                    .iter()
                    .all(|e| e.code == menu::LOOK || e.reason == Some(Reason::Window))
            );
        }
        let mut e = prompt_engine(c.clone(), &files);
        let (icon, run) = Run::start(&e.state(), menu::GET, at).expect("Get");
        let (mut queued, mut run) = (icon.to_vec(), Some(run));
        for _ in 0..200 {
            page_step(&mut e, &mut queued, &mut run);
        }
        let waits = menu::target_read(&e.state(), menu::GET);
        eprintln!("{d} cells off: still waiting {}", u8::from(waits));
        assert!(run.is_none(), "{d}: the press went in");
        assert_eq!(
            waits,
            d == 6,
            "{d}: the press gave the command nothing only outside"
        );
        assert!(still_there(&c, &e.state(), loaf), "{d}: not taken");
    }
}

/// The party aboard (A4-0x7b7a = the party size, the leader's slot the vessel's), as the
/// original's boarding leaves it.
fn aboard(s: &State) -> State {
    let mut a = s.clone();
    let size = usize::from(a.actors.party.size);
    a.actors.party.members[size] = a.actors.party.members[0];
    a.actors.party.leader_index = size as u8;
    a
}

/// Spec 17 round 2, item 12, aboard: on a loaf by the leader the menu disables Get, Move and Attack
/// as aboard, and run, each refuses before its target (the loaf stays, the leader's movement
/// points stay); in the panel, Drop is disabled aboard and run, the loaf stays carried. On foot
/// Get takes it, Move pushes it one cell (the direction key), Attack names it its target, Drop puts
/// it down (a press on a free cell by the leader). Cast's pair, one book-equipped state on foot
/// and aboard, is [`cast_with_the_same_book_runs_on_foot_and_is_refused_aboard`].
#[test]
fn aboard_the_commands_that_refuse_first_are_disabled_and_do_nothing() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let (loaf, at) = loaf_beside(&mut s);
    let hero = leader_slot(&s);
    let on_board = aboard(&s);
    let m = menu_of(&on_board, &files, at).expect("a menu");
    // What each command does once it has its target: Get and Move change the loaf, Attack names
    // it its target (A4-0x7a4a).
    let acted = |b: &State, a: &State| !still_there(b, a, loaf) || a.main.attack_target == loaf;
    for code in [menu::GET, menu::MOVE, menu::ATTACK] {
        assert_eq!(
            reason(&m, loaf, code),
            Some(Some(Reason::Aboard)),
            "{code:#x}"
        );
        let mut c = on_board.clone();
        c.main.attack_target = -1;
        let mut e = prompt_engine(c.clone(), &files);
        choose_then(&mut e, code, at, &[&[InputEvent::Key(Key::Right)]]);
        let did = acted(&c, &e.state());
        eprintln!("{code:#x} aboard: acted {}", u8::from(did));
        assert!(!did, "{code:#x}: nothing done aboard");
    }
    let m = menu_of(&s, &files, at).expect("a menu");
    let (lx, ly) = leader_at(&s);
    let dir = |dx: i16, dy: i16| match (dx, dy) {
        (1, 0) => Key::Right,
        (-1, 0) => Key::Left,
        (0, 1) => Key::Down,
        _ => Key::Up,
    };
    let (ox, oy) = (s.objects.x(loaf) as i16, s.objects.y(loaf) as i16);
    let push = dir(ox - lx as i16, oy - ly as i16);
    for (code, then) in [
        (menu::GET, vec![]),
        (menu::MOVE, vec![InputEvent::Key(push)]),
        (menu::ATTACK, vec![]),
    ] {
        assert_eq!(reason(&m, loaf, code), Some(None), "{code:#x} on foot");
        let mut c = s.clone();
        c.main.attack_target = -1;
        let mut e = prompt_engine(c.clone(), &files);
        assert!(
            choose_then(&mut e, code, at, &[&then]),
            "{code:#x}: its target read"
        );
        let did = acted(&c, &e.state());
        eprintln!("{code:#x} on foot: acted {}", u8::from(did));
        assert!(did, "{code:#x}: it acted");
    }
    let mut carried = s.clone();
    let held = crate::objects::create_in(&mut carried, 0x67, crate::objects::IN_INVENTORY, 1, hero);
    let at_held = cell_of_carried(&mut carried, held);
    let (tx, ty) = beside_tile(&carried, lx, ly).expect("a free tile by the leader");
    let free = cell_pixel(&carried, tx, ty);
    let spot = menu::left_press(free.0, free.1);
    for (label, st, refused) in [
        ("aboard", aboard(&carried), true),
        ("on foot", carried.clone(), false),
    ] {
        let m = menu_of(&st, &files, at_held).expect("a panel menu");
        let r = reason(&m, held, menu::DROP).expect("Drop shown");
        let mut e = prompt_engine(st.clone(), &files);
        choose_then(&mut e, menu::DROP, at_held, &[&spot]);
        let dropped = !carried_by(&e.state(), hero, held);
        eprintln!(
            "Drop {label}: enabled {}, dropped {}",
            u8::from(r.is_none()),
            u8::from(dropped)
        );
        assert_eq!(r, refused.then_some(Reason::Aboard), "Drop {label}");
        assert_eq!(r.is_none(), dropped, "Drop {label}");
    }
}

/// Spec 17 round 2, item 12, solo mode (A4's solo member, objlist tail 0x79): Talk is refused
/// while a member other than the first is the solo one (0x114afa: disabled as solo; run, no
/// conversation starts) and runs out of solo mode (a conversation starts); Drop takes no solo test
/// (the leader solo: enabled, and it drops).
#[test]
fn solo_mode_disables_talk_and_leaves_drop() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let hero = leader_slot(&s);
    let (lx, ly) = leader_at(&s);
    let free: Vec<(u16, u16)> = [
        (-1i32, 0i32),
        (1, 0),
        (0, 1),
        (0, -1),
        (-1, 1),
        (1, 1),
        (-1, -1),
        (1, -1),
    ]
    .into_iter()
    .map(|(dx, dy)| (i32::from(lx) + dx, i32::from(ly) + dy))
    .filter(|&(x, y)| walkable(&s, x, y))
    .map(|(x, y)| (x as u16, y as u16))
    .collect();
    assert!(free.len() >= 2, "two free tiles by the leader");
    let ((cx, cy), (tx, ty)) = (free[0], free[1]);
    let z = u16::from(s.map.centre.z);
    let npc = (1..FIRST_TEMP_ACTOR)
        .find(|&a| {
            let o = &s.objects;
            o.type_frame[a as usize] != 0
                && o.place(a) == 0
                && s.actors.attrs.status[a as usize] & 0x96 == 0
                && crate::converse::start::refusal(&s, hero, a).is_none()
                && crate::converse::start::may_have_script(a, o.kind(a))
        })
        .expect("a townsperson who would talk");
    crate::objects::move_to(&mut s, npc, cx, cy, z);
    crate::map::view::update_view(&mut s);
    let at = at_slot(&s, npc);
    let mut solo = s.clone();
    solo.actors.objlist_tail[0x79] = 2;
    for (label, st, refused) in [("solo", &solo, true), ("party", &s, false)] {
        let m = menu_of(st, &files, at).expect("a menu");
        let r = reason(&m, npc, menu::TALK).expect("Talk shown");
        let mut e = prompt_engine(st.clone(), &files);
        let (icon, run) = Run::start(&e.state(), menu::TALK, at).expect("Talk");
        let (mut queued, mut run) = (icon.to_vec(), Some(run));
        let mut talked = false;
        for _ in 0..600 {
            page_step(&mut e, &mut queued, &mut run);
            talked |= e.state().converse.active;
            if run.is_none() && (talked || at_prompt(&e, &queued)) {
                break;
            }
        }
        eprintln!(
            "Talk {label}: enabled {}, talked {}",
            u8::from(r.is_none()),
            u8::from(talked)
        );
        assert_eq!(r, refused.then_some(Reason::Solo), "Talk {label}");
        assert_eq!(r.is_none(), talked, "Talk {label}");
    }
    // Solo mode as `0x31`..`0x38` leave it for the leader (0x104b..: the tail byte its party index).
    let mut solo = s.clone();
    solo.actors.objlist_tail[0x79] = solo.actors.party.leader_index;
    let held = crate::objects::create_in(&mut solo, 0x67, crate::objects::IN_INVENTORY, 1, hero);
    let at_held = cell_of_carried(&mut solo, held);
    let m = menu_of(&solo, &files, at_held).expect("a panel menu");
    assert_eq!(reason(&m, held, menu::DROP), Some(None), "Drop in solo");
    let spot = cell_pixel(&solo, tx, ty);
    let mut e = prompt_engine(solo.clone(), &files);
    choose_then(
        &mut e,
        menu::DROP,
        at_held,
        &[&menu::left_press(spot.0, spot.1)],
    );
    assert!(!carried_by(&e.state(), hero, held), "Drop in solo drops");
}

/// Spec 17 round 2, item 12, Move's strength contest (0x115c06) is a draw, never judged before:
/// a townsperson standing next to the leader keeps Move enabled whether the seed lets the push
/// through or not; the real Move pushes it in one seed and not in the other.
#[test]
fn moves_contest_is_never_judged_before() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let (lx, ly) = leader_at(&s);
    let free = |s: &State, x: i32, y: i32| walkable(s, x, y);
    let (cx, cy, key) = [
        (1i32, 0i32, Key::Right),
        (-1, 0, Key::Left),
        (0, 1, Key::Down),
        (0, -1, Key::Up),
    ]
    .into_iter()
    .map(|(dx, dy, k)| (i32::from(lx) + dx, i32::from(ly) + dy, k, dx, dy))
    .find(|&(x, y, _, dx, dy)| free(&s, x, y) && free(&s, x + dx, y + dy))
    .map(|(x, y, k, _, _)| (x as u16, y as u16, k))
    .expect("two free cells in a line from the leader");
    let z = u16::from(s.map.centre.z);
    let npc = (1..FIRST_TEMP_ACTOR)
        .find(|&a| {
            let o = &s.objects;
            o.type_frame[a as usize] != 0
                && o.place(a) == 0
                && o.kind(a) != 0x153
                && s.actors.attrs.status[a as usize] & 0x80 == 0
                && !super::push::immovable(&s, a)
        })
        .expect("a townsperson");
    crate::objects::move_to(&mut s, npc, cx, cy, z);
    let t = s.objects.tf(npc);
    s.objects.type_frame[npc as usize] = t & !(3 << 10);
    crate::map::view::update_view(&mut s);
    let at = at_slot(&s, npc);
    let (mut won, mut lost) = (0, 0);
    for seed in 0..200u16 {
        let mut c = s.clone();
        c.rng.seed(seed);
        let m = menu_of(&c, &files, at).expect("a menu");
        assert_eq!(
            reason(&m, npc, menu::MOVE),
            Some(None),
            "seed {seed}: enabled"
        );
        let mut e = prompt_engine(c.clone(), &files);
        choose_then(&mut e, menu::MOVE, at, &[&[InputEvent::Key(key)]]);
        if still_there(&c, &e.state(), npc) {
            lost += 1;
        } else {
            won += 1;
        }
        if won > 0 && lost > 0 {
            break;
        }
    }
    eprintln!("seeds: pushed {won}, refused {lost}");
    assert!(
        won > 0 && lost > 0,
        "the draw went both ways, the entry enabled each time"
    );
}

/// The words 0x11a470 (Locate, the sextant's action, `magic::locate`) prints when run alone on
/// `st`: the text it adds to the message window, its spacing collapsed.
fn locate_says(st: &State, files: &Files) -> String {
    let words = |lines: Vec<String>| {
        lines
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let before = words(lines_of(st));
    let mut e = Engine::with_game(st.clone(), files.clone(), |st, io| async move {
        crate::magic::locate(&st, &io).await;
        crate::main_loop::flush_prints(&st, &io).await;
    });
    for _ in 0..50 {
        e.tick(&[], 16);
    }
    let after = words(lines_of(&e.state()));
    let added = after
        .strip_prefix(&before)
        .expect("Locate adds to the window's text");
    added.trim().to_string()
}

/// What a Use case's own action leaves (the engine after the run, the state before it, the use's
/// object, the lines printed since the run began).
type Reached = Box<dyn Fn(&Engine, &State, Slot, &[String]) -> bool>;

/// One fixture of a Use family: its state, its object, the menu's answer, and for a refused one
/// the line the handler gives instead of the action.
struct UseCase {
    label: &'static str,
    state: State,
    obj: Slot,
    want: Option<Reason>,
    line: Vec<u8>,
}

/// Spec 17 rounds 2, 3 and 5, items 4b, 7, 20, 5 and 12: every refusal of Use's case switch
/// (0x1305e0, `usecmd::cases::refusal`), family by family, both ways on matched fixtures: the
/// refused one has its exact reason (Case, or Solo for the ladder) and the handler gives its
/// refusal line and never its action; the allowed one is enabled and the handler reaches the
/// action, shown by the action's own checkpoint: the cube (flag clear, its eight moonstones, both
/// lenses and the Codex placed, the party near) reaches the ending's unported step (its stub hit);
/// a readied wand with one charge spends it; a carried orb says it cannot be worked out (the talk
/// flag clear); a sextant prints exactly what Locate (0x11a470) alone prints there, its print
/// sites in order ([`locate_says`]); a balloon basket is inflated (it goes); a chest on the map
/// opens (frame 1 to 0); a raft beside the leader is boarded; a readied shovel asks its direction;
/// a brazier is lit or doused; a ladder is climbed (the level changes).
#[test]
fn every_use_case_family_refuses_and_reaches_its_action_as_use_does() {
    use crate::usecmd::defs;
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let hero = leader_slot(&s);
    let io = Io::new(files.clone());
    let a4 = |off: u32| crate::main_loop::pointed_string(&io, crate::image::A4.wrapping_sub(off));
    let not_usable = a4(defs::MSG_NOT_USABLE);
    let no_effect = a4(defs::MSG_NO_EFFECT);
    let not_readied = super::text::string(&s, 0x0012_cfd4);
    let not_solo = super::text::string(&s, 0x0012_cfc0);
    let cant_figure = super::text::string(&s, 0x0012_ce42);
    let direction = super::text::string(&s, 0x0012_cd00);
    let refusals = [&not_usable, &not_readied, &not_solo];
    let (lx, ly) = leader_at(&s);
    let (cx, cy) = beside_tile(&s, lx, ly).expect("a free tile by the leader");
    let z = u16::from(s.map.centre.z);
    let made = |st: &State, f: &dyn Fn(&mut State) -> Slot| {
        let mut st = st.clone();
        let obj = f(&mut st);
        assert!(obj >= 0, "made");
        crate::map::view::update_view(&mut st);
        (st, obj)
    };
    let on_map = |tf: u16| {
        move |st: &mut State| crate::objects::create_on_map(&mut st.objects, tf, 0, 0, cx, cy, z)
    };
    let carried = |tf: u16, place: u8| {
        move |st: &mut State| crate::objects::create_in(st, tf, place, 1, hero)
    };
    let refused = |label, (state, obj): (State, Slot), want, line: &[u8]| UseCase {
        label,
        state,
        obj,
        want: Some(want),
        line: line.to_vec(),
    };
    let allowed = |label, (state, obj): (State, Slot)| UseCase {
        label,
        state,
        obj,
        want: None,
        line: Vec::new(),
    };
    let said =
        |text: Vec<u8>| -> Reached { Box::new(move |_, _, _, lines| said_line(lines, &text)) };
    let mut families: Vec<(Vec<UseCase>, Reached)> = Vec::new();

    // The cube (0x3e): the flag A4-0x7afb set refuses (no effect) before the Codex is looked for.
    let mut shrine = s.clone();
    if shrine.map.centre.z != 0 {
        shrine.map.centre.z = 0;
    }
    stand_at(&mut shrine, 0x39b, 0x355);
    for (kind, x) in [(0x18a, 0x399), (0x18c, 0x39d), (0x3b, 0x39b)] {
        assert!(crate::objects::create_on_map(&mut shrine.objects, kind, 0, 0, x, 0x353, 0) >= 0);
    }
    let cube = crate::objects::create_in(&mut shrine, 0x3e, crate::objects::IN_INVENTORY, 1, hero);
    for f in 0..8u16 {
        assert!(
            crate::objects::create_in(
                &mut shrine,
                0x56 | f << 10,
                crate::objects::IN_CONTAINER,
                1,
                cube
            ) >= 0
        );
    }
    shrine.map.flag_7afb = 0;
    crate::map::view::update_view(&mut shrine);
    let mut flagged = shrine.clone();
    flagged.map.flag_7afb = 1;
    families.push((
        vec![
            refused(
                "the cube, flag set",
                (flagged, cube),
                Reason::Case,
                &no_effect,
            ),
            allowed("the cube at the Codex, flag clear", (shrine, cube)),
        ],
        Box::new(|e, _, _, _| e.stub_hits().keys().any(|k| k.starts_with("0x13513a"))),
    ));
    // The wand (0x4e), each with one charge (0x150, quality 0x09): on the map not usable, carried
    // not readied; readied, 0x12d204 casts the charge's spell and takes the charge (0x10d16e).
    let charged = |place: Option<u8>| {
        move |st: &mut State| {
            let w = match place {
                None => crate::objects::create_on_map(&mut st.objects, 0x4e, 0, 0, cx, cy, z),
                Some(p) => crate::objects::create_in(st, 0x4e, p, 1, hero),
            };
            let charge = crate::objects::create_in(
                st,
                0x150,
                crate::objects::IN_CONTAINER,
                0x09 << 8 | 1,
                w,
            );
            assert!(charge >= 0, "the wand's charge");
            w
        }
    };
    let charges = |st: &State, wand: Slot| {
        (0x100..crate::objects::OBJECT_SLOTS as Slot)
            .filter(|&o| st.objects.kind(o) == 0x150 && st.objects.contained_in(o, wand))
            .count()
    };
    families.push((
        vec![
            refused(
                "a wand on the map",
                made(&s, &charged(None)),
                Reason::Case,
                &not_usable,
            ),
            refused(
                "a wand carried",
                made(&s, &charged(Some(crate::objects::IN_INVENTORY))),
                Reason::Case,
                &not_readied,
            ),
            allowed(
                "a wand readied",
                made(&s, &charged(Some(crate::objects::READIED))),
            ),
        ],
        Box::new(move |e, b, obj, _| charges(&e.state(), obj) + 1 == charges(b, obj)),
    ));
    // The orb (0x57): on the map not usable; carried, the talk flag clear, it cannot be worked out.
    let mut unaware = s.clone();
    if let Some(f) = unaware.actors.attrs.talk_flags.get_mut(5) {
        *f &= !0x20;
    }
    families.push((
        vec![
            refused(
                "an orb on the map",
                made(&unaware, &on_map(0x57)),
                Reason::Case,
                &not_usable,
            ),
            allowed(
                "an orb carried",
                made(&unaware, &carried(0x57, crate::objects::IN_INVENTORY)),
            ),
        ],
        said(cant_figure),
    ));
    // The sextant (0x5d) and the balloon basket (0x1a4): off levels 0 and 5 not usable.
    let level = |st: &State, l: u8| {
        let mut st = st.clone();
        st.map.centre.z = l;
        let p = st.objects.location[hero as usize];
        crate::objects::move_to(&mut st, hero, p.x, p.y, u16::from(l));
        st
    };
    let (surface, below) = (level(&s, 0), level(&s, 1));
    let sextant = carried(0x5d, crate::objects::IN_INVENTORY);
    // The sextant's own print: what 0x11a470 alone prints for the same place, its four print
    // sites in their order, run by itself on the same state.
    let located = locate_says(&surface, &files);
    assert!(
        located.chars().any(|c| c.is_ascii_digit()),
        "the place printed"
    );
    families.push((
        vec![
            refused(
                "a sextant below",
                made(&below, &sextant),
                Reason::Case,
                &not_usable,
            ),
            allowed("a sextant on the surface", made(&surface, &sextant)),
        ],
        said(located.into_bytes()),
    ));
    let basket = carried(0x1a4, crate::objects::IN_INVENTORY);
    families.push((
        vec![
            refused(
                "a basket below",
                made(&below, &basket),
                Reason::Case,
                &not_usable,
            ),
            allowed("a basket on the surface", made(&surface, &basket)),
        ],
        Box::new(move |e, _, obj, _| {
            e.state().objects.kind(obj) != 0x1a4 || !carried_by(&e.state(), hero, obj)
        }),
    ));
    // A chest (0x62, frame 1: closed) carried is not usable; on the map it opens.
    families.push((
        vec![
            refused(
                "a chest carried",
                made(&s, &carried(0x62 | 1 << 10, crate::objects::IN_INVENTORY)),
                Reason::Case,
                &not_usable,
            ),
            allowed("a chest on the map", made(&s, &on_map(0x62 | 1 << 10))),
        ],
        Box::new(|e, _, obj, _| e.state().objects.tf(obj) >> 10 == 0),
    ));
    // A vessel (0x19f, a raft; on the map an object, quality 0: no deed) carried is not usable;
    // beside the leader it is boarded (the party aboard: A4-0x7b7a = the party size).
    families.push((
        vec![
            refused(
                "a raft carried",
                made(&s, &carried(0x19f, crate::objects::IN_INVENTORY)),
                Reason::Case,
                &not_usable,
            ),
            allowed("a raft beside the leader", made(&s, &on_map(0x19f))),
        ],
        Box::new(|e, _, _, _| {
            let p = &e.state().actors.party;
            p.leader_index == p.size
        }),
    ));
    // The shovel (0x67): carried not readied; readied it digs (asks the direction, 0x12cd00).
    families.push((
        vec![
            refused(
                "a shovel carried",
                made(&s, &carried(0x67, crate::objects::IN_INVENTORY)),
                Reason::Case,
                &not_readied,
            ),
            allowed(
                "a shovel readied",
                made(&s, &carried(0x67, crate::objects::READIED)),
            ),
        ],
        said(direction),
    ));
    // The brazier (0xce): frame bit 1 not usable; clear, it is lit or doused (its frame changes).
    families.push((
        vec![
            refused(
                "a brazier, frame bit 1",
                made(&s, &on_map(0xce | 2 << 10)),
                Reason::Case,
                &not_usable,
            ),
            allowed("a brazier, frame bit clear", made(&s, &on_map(0xce))),
        ],
        Box::new(|e, b, obj, _| e.state().objects.tf(obj) != b.objects.tf(obj)),
    ));
    // The ladder (0x131): solo it refuses; in party it is climbed (the level changes).
    let mut solo = s.clone();
    solo.actors.objlist_tail[0x79] = 0;
    families.push((
        vec![
            refused(
                "a ladder in solo",
                made(&solo, &on_map(0x131)),
                Reason::Solo,
                &not_solo,
            ),
            allowed("a ladder in party", made(&s, &on_map(0x131))),
        ],
        Box::new(|e, b, _, _| e.state().map.centre.z != b.map.centre.z),
    ));

    for (cases, reached) in &families {
        for case in cases {
            let UseCase {
                label,
                state,
                obj,
                want,
                line,
            } = case;
            let mut st = state.clone();
            let at = if st.objects.place(*obj) == 0 {
                at_slot(&st, *obj)
            } else {
                cell_of_carried(&mut st, *obj)
            };
            let m = menu_of(&st, &files, at).expect("a menu");
            let r = reason(&m, *obj, menu::USE).expect("Use shown");
            let mut e = prompt_engine(st.clone(), &files);
            let before = lines_of(&e.state()).len();
            choose(&mut e, menu::USE, at);
            let lines = lines_of(&e.state());
            let new = &lines[before.saturating_sub(1).min(lines.len())..];
            let did = reached(&e, &st, *obj, new);
            let refusal = refusals.iter().any(|l| said_line(new, l));
            let its_line = !line.is_empty() && said_line(new, line);
            eprintln!(
                "{label}: enabled {}, its action {}, a refusal line {}",
                u8::from(r.is_none()),
                u8::from(did),
                u8::from(refusal || its_line)
            );
            assert_eq!(r, *want, "{label}: the menu");
            assert_eq!(did, want.is_none(), "{label}: the handler's action");
            if want.is_some() {
                assert!(its_line, "{label}: its refusal line");
            } else {
                assert!(!refusal, "{label}: no refusal line");
            }
        }
    }
}

/// The words of `text` (a line of the user's `game`), in order, among the words of `lines`.
fn said_line(lines: &[String], text: &[u8]) -> bool {
    let words = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    let want = words(&String::from_utf8_lossy(text));
    !want.is_empty() && words(&lines.join(" ")).contains(&want)
}

/// Spec 17 rounds 2 and 3, items 13b and 13: a more prompt before the target read. The message
/// window is paged with one row left when Attack starts (its name and weapon line, 0x124caa, fill
/// it): the more prompt waits (A4-0x7a42 clear) with Attack's target pending. The held press is
/// released exactly once, never while the more prompt is up (a key answers it), and lands once
/// the target read is up: the attack target (a sentinel, -1, before) is the loaf, and the prompt
/// comes back.
#[test]
fn the_target_press_waits_through_a_more_prompt() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let (_, (bx, by)) = closed_door(&s);
    stand_at(&mut s, bx, by);
    let (loaf, at) = loaf_beside(&mut s);
    armed(&mut s, 0);
    s.main.attack_target = -1;
    let (mut e, st) = shared_engine(s.clone(), &files);
    {
        let mut w = st.borrow_mut();
        w.ui.text.win_mut().flags |= crate::ui::text::PAGED;
        w.ui.text.more_rows = 1;
    }
    let (icon, run) = Run::start(&e.state(), menu::ATTACK, at).expect("Attack");
    let (mut queued, mut run) = (icon.to_vec(), Some(run));
    let (mut mores, mut released, mut released_in_more, mut back) = (0, 0, 0, false);
    for _ in 0..600 {
        let more = {
            let w = st.borrow();
            !w.ui.input.accept && w.ui.input.in_wait && w.ui.input.target_pending == 1
        };
        if more && run.is_some() && queued.is_empty() && e.io().pending_events() == 0 {
            queued.push(InputEvent::Key(Key::Space));
            mores += 1;
        }
        let in_more = more;
        if page_step(&mut e, &mut queued, &mut run) {
            released += 1;
            released_in_more += usize::from(in_more);
        }
        if run.is_none() && at_prompt(&e, &queued) {
            back = true;
            break;
        }
    }
    eprintln!(
        "more prompts answered {mores}, releases {released}, in a more prompt {released_in_more}"
    );
    assert!(mores >= 1, "a more prompt came before the target read");
    assert_eq!(
        (released, released_in_more),
        (1, 0),
        "one release, none in the more prompt"
    );
    assert!(back, "the prompt came back");
    assert_eq!(
        e.state().main.attack_target,
        loaf,
        "the press reached Attack's target read"
    );
}

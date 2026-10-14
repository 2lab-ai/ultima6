//! Real data: the user's archive.org zip → u6-intake → `Files` → `State::new_game`, then the
//! object commands. Skips when the zip is absent; reads only, prints slots and counts only (no
//! game text: the scroll lines are shown by count and length).
#![allow(clippy::disallowed_methods)] // the test reads the zip

use std::cell::RefCell;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;

use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::objects::{ACTOR_SLOTS, FIRST_TEMP_ACTOR, Slot};
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

pub(super) fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// The state the command loop starts from: load state and its tail (the region, the window
/// and its egg pass), RNG seeded with `seed` first as the engine does.
pub(super) fn started(files: &Files, seed: u16) -> State {
    let mut s = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
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
    slot: Slot,
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
        slot: egg,
        at: (s.objects.x(egg), s.objects.y(egg)),
        templates,
        full: s.objects.qq_of(egg) & 0xff == 100,
    }
}

/// View cells (column, row of the 11 x 11 view around the centre) of the monsters, sorted.
fn cells(s: &State) -> Vec<(i16, i16)> {
    let c = s.map.centre;
    let mut v: Vec<(i16, i16)> = monsters(s)
        .iter()
        .map(|m| (m.2 as i16 - c.x as i16 + 5, m.3 as i16 - c.y as i16 + 5))
        .collect();
    v.sort_unstable();
    v
}

/// View cells given as `col,row` pairs separated by spaces in the environment variable `var`
/// (an oracle frame's cells, kept outside the repository), sorted.
fn cells_from_env(var: &str) -> Option<Vec<(i16, i16)>> {
    let text = std::env::var(var).ok()?;
    let mut v: Vec<(i16, i16)> = text
        .split_whitespace()
        .map(|p| {
            let (x, y) = p.split_once(',').expect("col,row");
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect();
    v.sort_unstable();
    Some(v)
}

#[test]
fn the_start_egg_hatches_its_gargoyles() {
    let Some(files) = files() else { return };
    let mut s = started(&files, 0);
    let c = s.map.centre;
    let ms = monsters(&s);
    let Egg {
        slot: egg,
        at,
        templates,
        full,
    } = hatched_egg(&mut s);
    eprintln!(
        "egg {egg:#x}: {} templates, quantity 100 {full}; {} monsters",
        templates.len(),
        ms.len()
    );
    for &(m, t, x, y, z) in &ms {
        let i = m as usize;
        let a = &s.actors.attrs;
        eprintln!(
            "monster {m:#x} type {t:#x} view cell ({}, {}) z {z} str {} dex {} int {} hp {} lvl {} \
             status {:#04x} worktype {:#x} mp {}",
            x as i16 - c.x as i16 + 5,
            y as i16 - c.y as i16 + 5,
            a.strength[i],
            a.dexterity[i],
            a.intelligence[i],
            a.hp[i],
            a.level[i],
            a.status[i],
            s.actors.worktype[i],
            s.actors.movement_points[i],
        );
        let mut ch = s.objects.next_of(m);
        while ch >= 0 && s.objects.place(ch) != 0 && s.objects.contained_in(ch, m) {
            eprintln!(
                "   carries {ch:#x} type {:#x} status {:#04x} qq {:#06x}",
                s.objects.kind(ch),
                s.objects.status(ch),
                s.objects.qq_of(ch)
            );
            ch = s.objects.next_of(ch);
        }
    }
    // Each template makes 1..=its quantity of its type (all of it from an egg of quantity 100),
    // gargoyles among them (0x16a / 0x16b, the types 0x1319b2 compares with).
    assert!(!ms.is_empty(), "the egg hatched monsters");
    for &(kind, n) in &templates {
        let made = ms.iter().filter(|m| m.1 == kind).count() as u16;
        assert!(made <= n, "template {kind:#x}: {made} of at most {n}");
        assert!(
            !full || made == n,
            "template {kind:#x}: the whole {n} of a full egg"
        );
    }
    assert!(ms.iter().all(|m| templates.iter().any(|t| t.0 == m.1)));
    assert!(ms.iter().any(|m| matches!(m.1, 0x16a | 0x16b)));
    // The first stays on the egg's tile.
    assert!(ms.iter().any(|m| (m.2, m.3) == at), "one on the egg's tile");
    // Seed 0x1d3: the seed [`search_seeds_for_the_oracle_gargoyles`] found for the view cells an
    // oracle frame of the original's start shows (the oracle's own seed, the low word of
    // `time()`, is unknown). The test checks that they are in view on cells of their own, and
    // the oracle's cells when `U6_ORACLE_GARGOYLE_CELLS` holds them (kept outside the repo).
    let s = started(&files, 0x1d3);
    let got = cells(&s);
    let mut distinct = got.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), got.len(), "one monster a cell");
    assert!(
        got.iter()
            .all(|&(x, y)| (0..11).contains(&x) && (0..11).contains(&y))
    );
    if let Some(want) = cells_from_env("U6_ORACLE_GARGOYLE_CELLS") {
        assert!(got == want, "the oracle frame's cells");
    }
}

/// Seed search (run by hand with `--ignored`): every seed word whose start hatch puts the
/// monsters on the view cells an oracle frame shows, given as `U6_ORACLE_GARGOYLE_CELLS="c,r
/// c,r ..."` (read off the frame; the cells are the user's world, so they stay out of the repo).
#[test]
#[ignore]
fn search_seeds_for_the_oracle_gargoyles() {
    let Some(files) = files() else { return };
    let Some(want) = cells_from_env("U6_ORACLE_GARGOYLE_CELLS") else {
        eprintln!("skipped: U6_ORACLE_GARGOYLE_CELLS not set");
        return;
    };
    let mut base = State::new_game(&files).unwrap_or_else(|e| panic!("{e}"));
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
    eprintln!("{} seeds: {:x?}", hits.len(), hits);
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
    let mut last: String = t.transcript_line.iter().map(|&b| char::from(b)).collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| char::from(b)));
    lines.push(last);
    lines
}

type Command = Box<dyn FnOnce(Rc<RefCell<State>>, Rc<Io>) -> Pin<Box<dyn Future<Output = ()>>>>;

/// Runs one command handler on `s`: `keys` first, then Space whenever the game idles on a key
/// (more prompts). Returns the state after it, the new scroll lines and the stub hits.
fn run(s: State, files: &Files, keys: &[InputEvent], cmd: Command) -> (State, Vec<String>) {
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
    for (id, n) in e.stub_hits() {
        eprintln!("   stub {id} x{n}");
    }
    let s = e.state().clone();
    let lines = lines_of(&s);
    let new = lines[before.saturating_sub(1).min(lines.len())..].to_vec();
    (s, new)
}

/// Points the cross-hair at `slot` as a confirmed target does: selection, map target and the
/// pointer on its view cell.
fn aim(s: &mut State, slot: Slot) {
    let c = s.map.centre;
    let (x, y) = (s.objects.x(slot), s.objects.y(slot));
    let col = x as i16 - c.x as i16 + 5;
    let row = y as i16 - c.y as i16 + 5;
    let input = &mut s.ui.input;
    input.selection = slot;
    input.target = (x as i16, y as i16);
    input.pointer = (col * 16 + 8, row * 16 + 8);
    input.target_void = false;
}

/// Puts the leader (the Avatar) at (x, y) with the view on it.
fn stand_at(s: &mut State, x: u16, y: u16) {
    let z = u16::from(s.map.centre.z);
    crate::objects::move_to(s, 1, x, y, z);
    s.map.centre.x = x;
    s.map.centre.y = y;
    crate::map::region::refresh_region(s);
    crate::map::view::update_view(s);
}

fn print_lines(what: &str, lines: &[String]) {
    let lengths: Vec<usize> = lines.iter().map(String::len).collect();
    eprintln!("{what}: {} scroll lines, lengths {lengths:?}", lines.len());
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
    let (x, y) = (i32::from(s.objects.x(slot)), i32::from(s.objects.y(slot)));
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

/// True when `slot` lies outside the 11 x 11 view around the centre.
fn out_of_view(s: &State, slot: Slot) -> bool {
    let c = s.map.centre;
    let dx = i32::from(s.objects.x(slot)) - i32::from(c.x);
    let dy = i32::from(s.objects.y(slot)) - i32::from(c.y);
    dx.abs() > 5 || dy.abs() > 5
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

/// A tile next to the Avatar with no actor, on which the map lets an object lie (not a
/// blocking tile), south-east first.
fn drop_spot(s: &State) -> Option<(u16, u16)> {
    let (x, y) = (i32::from(s.objects.x(1)), i32::from(s.objects.y(1)));
    let z = u16::from(s.map.centre.z);
    let tf = &s.tables.tileflags;
    [
        (1, 1),
        (1, 0),
        (0, 1),
        (-1, 1),
        (1, -1),
        (-1, 0),
        (0, -1),
        (-1, -1),
    ]
    .iter()
    .filter_map(|(dx, dy)| Some((u16::try_from(x + dx).ok()?, u16::try_from(y + dy).ok()?)))
    .find(|&(x, y)| s.map.is_passable(tf, x, y, z) && crate::objects::actor_at(s, 0, x, y) < 0)
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
    let loaf_weight = i16::from(files.get("tileflag").unwrap()[0x1000 + 0x80]);
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
    eprintln!(
        "carried weight (readied, rest) {before:?} -> {after:?}; movement points {mp} -> {}",
        s.actors.movement_points[1]
    );
    // The loaf stacks into the Avatar's inventory (0x10cf7c merges it into a stack he has or
    // keeps this object); either way the Avatar now carries a loaf and the weight rose by its
    // own; Get takes 3 movement points (0x1285b6's).
    let loaf = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .find(|&o| s.objects.kind(o) == 0x80 && carried_by(&s, 1, o))
        .expect("a loaf in the Avatar's inventory");
    assert_eq!(
        after.1 - before.1,
        loaf_weight,
        "the weight rose by the loaf's"
    );
    assert_eq!(s.actors.movement_points[1], mp.wrapping_sub(3));

    // Drop it on a tile next to the Avatar where an object may lie (the inventory panel of
    // member 0 shows the loaf).
    s.ui.panel.mode = 0x92;
    s.ui.panel.member = 0;
    let (ax, ay) = drop_spot(&s).expect("a tile beside the Avatar to drop on");
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
    eprintln!("carried weight after the drop {back:?}");
    let on_floor = (0x100..crate::objects::OBJECT_SLOTS as Slot).find(|&o| {
        s.objects.kind(o) == 0x80
            && s.objects.place(o) == 0
            && (s.objects.x(o), s.objects.y(o)) == (ax, ay)
    });
    assert!(on_floor.is_some(), "the loaf lies on the table");
    assert_eq!(back, before);
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
    eprintln!(
        "chair ({cx}, {cy}) -> ({}, {})",
        s.objects.x(chair),
        s.objects.y(chair)
    );
    assert_eq!((s.objects.x(chair), s.objects.y(chair)), (cx, cy + 1));

    // The nearest closed steel door (0x12c, frames 4..7 closed): open, then closed again.
    let mut s = with_text(&files);
    let door = *nearest(&s, 0x12c, |s, o| s.objects.tf(o) >> 10 & 4 != 0)
        .first()
        .expect("a closed door near the start");
    let closed = s.objects.tf(door) >> 10;
    let mut frames = Vec::new();
    for _ in 0..2 {
        let f0 = s.objects.tf(door) >> 10;
        super::door(&mut s, door, false, false, false);
        let open = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
            Box::pin(async move { crate::main_loop::flush_prints(&st, &io).await })
                as Pin<Box<dyn Future<Output = ()>>>
        });
        let (s2, lines) = run(s, &files, &[], open);
        s = s2;
        print_lines(
            &format!("door frame {f0} -> {}", s.objects.tf(door) >> 10),
            &lines,
        );
        frames.push(s.objects.tf(door) >> 10);
    }
    assert_eq!(frames, [closed ^ 4, closed], "open, then closed");
}

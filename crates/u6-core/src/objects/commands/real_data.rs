//! Real data: the user's archive.org zip → u6-intake → `Files` → `State::new_game`, then the
//! object commands. Skips when the zip is absent; reads only, prints numbers and names only.
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

#[test]
fn the_throne_room_egg_hatches_three_winged_gargoyles_at_start() {
    let Some(files) = files() else { return };
    let s = started(&files, 0);
    let c = s.map.centre;
    let ms = monsters(&s);
    eprintln!("centre ({}, {}, {})", c.x, c.y, c.z);
    for &(m, t, x, y, z) in &ms {
        let i = m as usize;
        let a = &s.actors.attrs;
        eprintln!(
            "monster {m:#x} type {t:#x} at ({x}, {y}, {z}) view cell ({}, {}) str {} dex {} int {} \
             hp {} lvl {} status {:#04x} worktype {:#x} mp {}",
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
    assert_eq!(ms.len(), 3, "the egg's template asks for 3");
    assert!(ms.iter().all(|m| m.1 == 0x16b));
    // The first stays on the egg's tile.
    assert!(ms.iter().any(|m| (m.2, m.3) == (307, 350)));
    // Seed 0x1d3 is one of the 125 seed words (of 32768) whose hatch puts the three on the view
    // cells the oracle frame shows (shots/play-20261014-190647/ingame.png: (2, 3), (3, 3),
    // (5, 3)); the oracle's own seed (the low word of `time()`) is unknown.
    let s = started(&files, 0x1d3);
    let mut cells: Vec<(i16, i16)> = monsters(&s)
        .iter()
        .map(|m| (m.2 as i16 - c.x as i16 + 5, m.3 as i16 - c.y as i16 + 5))
        .collect();
    cells.sort();
    assert_eq!(cells, [(2, 3), (3, 3), (5, 3)]);
}

/// Seed search (run by hand with `--ignored`): every seed word whose start hatch puts the three
/// gargoyles on the view cells the oracle frame shows.
#[test]
#[ignore]
fn search_seeds_for_the_oracle_gargoyles() {
    let Some(files) = files() else { return };
    let mut base = State::new_game(&files).unwrap_or_else(|e| panic!("{e}"));
    crate::objects::init_slots(&mut base.objects);
    let want = [(304u16, 350u16), (305, 350), (307, 350)];
    let mut hits = Vec::new();
    for seed in (0..=0xffffu16).filter(|w| w & 1 == 1) {
        let mut s = base.clone();
        s.rng.seed(seed);
        crate::main_loop::load_state_tail(&mut s);
        let mut got: Vec<(u16, u16)> = monsters(&s).iter().map(|m| (m.2, m.3)).collect();
        got.sort();
        if got == want {
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
    eprintln!("{what}:");
    for l in lines {
        eprintln!("   | {l}");
    }
}

fn find(s: &State, kind: u16, dx: i16, dy: i16) -> Slot {
    let c = s.map.centre;
    (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .find(|&o| {
            s.objects.kind(o) == kind
                && s.objects.place(o) == 0
                && s.objects.x(o) as i16 - c.x as i16 == dx
                && s.objects.y(o) as i16 - c.y as i16 == dy
        })
        .unwrap_or_else(|| panic!("no type {kind:#x} at ({dx}, {dy})"))
}

fn look_cmd() -> Command {
    Box::new(|st, io| Box::pin(async move { super::look(&st, &io).await }))
}

#[test]
fn look_at_six_things_in_the_throne_room_and_the_kitchen() {
    let Some(files) = files() else { return };
    let s0 = with_text(&files);
    let picture = find(&s0, 0x8f, -5, 0);
    let brazier = find(&s0, 0xce, -2, -4);
    let cleaver = find(&s0, 0x71, 8, 3);
    let clock = find(&s0, 0x9f, -7, -2);
    let mut all = Vec::new();
    for (what, slot, at) in [
        ("Lord British", 5, None),
        ("a gargoyle", 0xe0, None),
        ("the picture (a sign: book.dat)", picture, None),
        ("a brazier", brazier, None),
        ("the cleaver (weight, damage)", cleaver, Some((314, 355))),
        ("the clock (time)", clock, Some((301, 350))),
    ] {
        let mut s = s0.clone();
        if let Some((x, y)) = at {
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

fn carried_by(s: &State, owner: Slot, slot: Slot) -> bool {
    s.objects.place(slot) != 0 && s.objects.contained_in(slot, owner)
}

#[test]
fn get_the_bread_then_drop_it_on_a_table() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let bread = find(&s, 0x80, 6, 0);
    let (bx, by) = (s.objects.x(bread), s.objects.y(bread));
    stand_at(&mut s, bx - 1, by);
    aim(&mut s, bread);
    let weight = |s: &mut State| {
        super::seams::sum_inventory(s, 1);
        s.objcmds.inventory_weights
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
    // keeps this object); either way the Avatar now carries a loaf and the weight rose by its 3.
    let loaf = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .find(|&o| s.objects.kind(o) == 0x80 && carried_by(&s, 1, o))
        .expect("a loaf in the Avatar's inventory");
    assert_eq!(after.1 - before.1, 3);
    assert_eq!(s.actors.movement_points[1], mp.wrapping_sub(3));

    // Drop it on the kitchen table south-east of the Avatar, where a portion of meat lies (the
    // inventory panel of member 0 shows it).
    s.ui.panel.mode = 0x92;
    s.ui.panel.member = 0;
    let (ax, ay) = (s.objects.x(1) + 1, s.objects.y(1) + 1);
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
    let chair = find(&s, 0xfc, -6, 0);
    let (cx, cy) = (s.objects.x(chair), s.objects.y(chair));
    stand_at(&mut s, cx + 1, cy);
    aim(&mut s, chair);
    let push = Box::new(|st: Rc<RefCell<State>>, io: Rc<Io>| {
        Box::pin(async move { super::push(&st, &io).await }) as Pin<Box<dyn Future<Output = ()>>>
    });
    // "To " then a direction: south.
    let (s, lines) = run(s, &files, &[InputEvent::Key(Key::Down)], push);
    print_lines("Move the chair south", &lines);
    eprintln!(
        "chair ({cx}, {cy}) -> ({}, {})",
        s.objects.x(chair),
        s.objects.y(chair)
    );
    assert_eq!((s.objects.x(chair), s.objects.y(chair)), (cx, cy + 1));

    // The steel door west of the throne room: closed (frame 5) → open → closed.
    let mut s = with_text(&files);
    let door = find(&s, 0x12c, -6, -5);
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
    }
    assert_eq!(s.objects.tf(door) >> 10, 5);
}

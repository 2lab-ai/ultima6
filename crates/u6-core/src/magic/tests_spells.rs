//! Real data: the spells ported in the second magic unit, cast from the start state of the
//! user's archive.org zip — through the book with the incantation typed (its letters read from
//! the user's `game`) and the cross-hair moved by arrow keys where it can aim, and with the
//! target set as 0x1225c0 would return it where it cannot (objects). Compares the message
//! scroll with strings read from the user's `game` (printing only its line count) and asserts
//! the state changes; the visual ones write frames to /tmp/claude-501/. Skips where the zip is
//! missing; no game data in the repo: places are offsets from the start's view centre (the
//! leader's place, read at run time), and the actors a spell is aimed at are put there first.
#![allow(clippy::disallowed_methods)] // tests read the zip and write the frame receipts

use super::tables::Msg;
use super::tests_real::{
    boot, cast_command, files, give_book, keys, lines_of, msg, png, printed, show, text_at,
};
use crate::engine::{Engine, Io};
use crate::files::Files;
use crate::main_loop::flush_prints;
use crate::objects::Slot;
use crate::state::State;
use std::cell::RefCell;

type Setup = Box<dyn FnOnce(&mut State)>;

/// The incantation letters of `spell` from the user's `game`.
fn word(files: &Files, spell: u8) -> String {
    let s = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
    let w = s.magic.tables.word(usize::from(spell));
    w.iter().map(|&b| char::from(b)).collect()
}

/// Boots, gives the Avatar a book with `spell` and the reagents, runs `setup`, casts: the
/// incantation, return, then `after` one key per tick. Returns the engine when it is over.
fn cast(files: &Files, spell: u8, setup: Setup, after: &str) -> Engine {
    let mut e = start(files, spell, setup, after);
    finish(&mut e);
    e
}

/// [`cast`] without waiting for the end.
fn start(files: &Files, spell: u8, setup: Setup, after: &str) -> Engine {
    let input = format!("{}\n{after}", word(files, spell));
    let state = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
    let mut e = Engine::with_game(state, files.clone(), move |st, io| async move {
        boot(&st, &io).await;
        {
            let mut s = st.borrow_mut();
            give_book(&mut s, &[spell]);
            setup(&mut s);
        }
        cast_command(&st, &io).await;
    });
    e.tick(&[], 16);
    for k in keys(&input) {
        e.tick(&[k], 16);
    }
    e
}

fn finish(e: &mut Engine) {
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished(), "still waits: {:?}", lines_of(&e.state()));
}

/// Runs `spell` through the dispatcher with its target chosen (0x11a586 with D1 = 1, as
/// 0x11abd0 runs it for a target picked before the book), after `setup` sets the selection
/// and the location the way 0x1225c0 would; `after` is typed one key per tick.
fn dispatch_chosen(files: &Files, spell: u16, setup: Setup, after: &str) -> Engine {
    let state = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
    let mut e = Engine::with_game(state, files.clone(), move |st, io| async move {
        boot(&st, &io).await;
        setup(&mut st.borrow_mut());
        cast_line(&st, &io).await;
        super::dispatch(&st, &io, spell, true).await;
        flush_prints(&st, &io).await;
    });
    e.tick(&[], 16);
    for k in keys(after) {
        e.tick(&[k], 16);
    }
    finish(&mut e);
    e
}

/// The Cast line as the command loop prints it (so [`show`] finds the start).
async fn cast_line(st: &RefCell<State>, io: &Io) {
    let at = crate::image::A4.wrapping_sub(0x7c8e) + 4 * 0x82;
    let name = crate::main_loop::pointed_string(io, at);
    let fmt = crate::main_loop::game_string(io, 0x0010_1f48);
    crate::ui::printf::printf(st, io, &fmt, &[crate::ui::printf::Arg::Str(&name)]).await;
}

fn place(s: &mut State, a: Slot, x: u16, y: u16) {
    let z = u16::from(s.map.centre.z);
    crate::objects::move_to(s, a, x, y, z);
}

/// The start state's view centre (the leader's place, read from the user's files).
fn start_centre(files: &Files) -> (u16, u16, u16) {
    let c = State::new_game(files).unwrap().map.centre;
    (c.x, c.y, u16::from(c.z))
}

/// The tile (dx, dy) from the view centre of `s`.
fn off(s: &State, dx: i16, dy: i16) -> (u16, u16) {
    let c = s.map.centre;
    (c.x.wrapping_add_signed(dx), c.y.wrapping_add_signed(dy))
}

/// Puts actor `a` on the tile (dx, dy) from the view centre.
fn place_at(s: &mut State, a: Slot, dx: i16, dy: i16) {
    let (x, y) = off(s, dx, dy);
    place(s, a, x, y);
}

/// True when `t` is within `r` tiles of (x, y) on both axes.
fn within(t: (u16, u16), (x, y, _): (u16, u16, u16), r: u16) -> bool {
    t.0.abs_diff(x) <= r && t.1.abs_diff(y) <= r
}

/// True when actor `a` of `s` is evil (alignment bits 5-6 = 1).
fn evil(s: &State, a: usize) -> bool {
    s.actors.attrs.status[a] & 0x60 == 0x20
}

/// True when 0x1231be never hurts actor `a`: its creature index is 0x1f (its hit points
/// become 0xff instead).
fn unhurt_kind(s: &State, a: usize) -> bool {
    crate::objects::monster_index(&s.objects, s.objects.type_frame[a] & 0x3ff) == 0x1f
}

fn at(s: &State, a: Slot) -> (u16, u16) {
    (s.objects.x(a), s.objects.y(a))
}

fn frame(e: &Engine, name: &str) {
    let path = format!("/tmp/claude-501/m2-magic2-{name}.png");
    std::fs::write(&path, png(e.frame())).unwrap();
    eprintln!("  frame {path}");
}

/// Intelligence 2: an actor that cannot resist the Avatar (0x122076: random 1..30 below
/// 2 / 2 − 15 + 15).
fn dull(s: &mut State, a: Slot) {
    s.actors.attrs.intelligence[a as usize] = 2;
}

/// Mass Sleep (0x26) on a group of three put around the tile two right and three up of the
/// centre, aimed with the cross-hair two right and three up.
#[test]
fn mass_sleep_on_a_group() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        place_at(s, 7, 2, -3);
        place_at(s, 6, 3, -3);
        place_at(s, 8, 2, -2);
        for a in [6, 7, 8] {
            dull(s, a);
        }
        crate::map::view::update_view(s);
    });
    let e = cast(&files, 0x26, setup, ">>^^^\n");
    let tail = show("Mass Sleep two right, three up", &e);
    let s = e.state();
    let asleep: Vec<Slot> = [6, 7, 8]
        .into_iter()
        .filter(|&a| s.actors.attrs.status[a as usize] & 4 != 0)
        .collect();
    eprintln!("  asleep: {asleep:?}");
    for a in [6usize, 7, 8] {
        eprintln!(
            "  actor {a}: tf {:#x} at {:?} status {:#x} int {}",
            s.objects.type_frame[a],
            s.objects.location[a],
            s.actors.attrs.status[a],
            s.actors.attrs.intelligence[a]
        );
    }
    assert_eq!(asleep, [6, 7, 8], "{tail:?}");
    assert_eq!(
        s.actors.attrs.status[1] & 4,
        0,
        "the caster is outside the blast"
    );
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Fire Field (0x33) one tile north-west of the Avatar (cross-hair one left, one up): the field
/// object lies there; Dispel Field (0x21) aimed at it (the object as 0x1225c0 would pick it)
/// takes it away again.
#[test]
fn fire_field_then_dispel_field() {
    let Some(files) = files() else { return };
    let c = start_centre(&files);
    let (fx, fy) = (c.0 - 1, c.1 - 1);
    let e = cast(&files, 0x33, Box::new(|_| {}), "<^\n");
    let tail = show("Fire Field one left, one up", &e);
    frame(&e, "fire-field");
    let field = {
        let mut s = e.state().clone();
        let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, fx, fy, c.2);
        while o >= 0 && s.objects.kind(o) != super::area::FIRE_FIELD {
            o = crate::objects::next_at(&mut s.objects, &s.tables);
        }
        o
    };
    eprintln!("  field object {field}");
    assert!(field >= 0x100, "a fire field one left, one up: {tail:?}");
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
    let before = e.state().clone();
    let setup: Setup = Box::new(move |s| {
        *s = before;
        s.ui.input.selection = field;
        s.ui.input.target = (fx as i16, fy as i16);
    });
    let e = dispatch_chosen(&files, 0x21, setup, "");
    let tail = show("Dispel Field on it", &e);
    let s = e.state();
    assert_eq!(s.objects.type_frame[field as usize], 0, "{tail:?}");
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Conjure (0x31): a creature of the five Conjure types appears within 5 of the Avatar on its
/// side (alignment 0x40, worktype 8).
#[test]
fn conjure_makes_a_creature() {
    let Some(files) = files() else { return };
    let c = start_centre(&files);
    let e = cast(&files, 0x31, Box::new(|_| {}), "");
    let tail = show("Conjure", &e);
    let s = e.state();
    let made: Vec<usize> = (0xe0..0x100)
        .filter(|&a| {
            [0x173, 0x156, 0x169, 0x158, 0x166].contains(&(s.objects.type_frame[a] & 0x3ff))
        })
        .collect();
    eprintln!("  conjured: {made:x?}");
    let &[a] = made.as_slice() else {
        panic!("one conjured creature: {made:?} {tail:?}");
    };
    let (x, y) = at(&s, a as Slot);
    eprintln!(
        "  at ({x}, {y}), status {:#x}, worktype {}",
        s.actors.attrs.status[a], s.actors.worktype[a]
    );
    assert!(within((x, y), c, 5), "within 5 of the caster");
    assert_eq!(s.actors.attrs.status[a] & 0x60, 0x40);
    assert_eq!(s.actors.worktype[a], 8);
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Telekinesis (0x15) on an object two tiles east of the Avatar (picked the way 0x1225c0
/// would), pushed one tile north with the up arrow.
#[test]
fn telekinesis_pushes_an_object() {
    let Some(files) = files() else { return };
    let c = start_centre(&files);
    let setup: Setup = Box::new(|s| {
        // The first type with a weight byte that is not a crank or a lever.
        let kind = (1..0x100u16)
            .find(|&k| s.tables.tileflags.weight[usize::from(k)] != 0 && k != 0x120 && k != 0x10c)
            .unwrap();
        let (x, y) = off(s, 2, 0);
        let z = u16::from(s.map.centre.z);
        let o = crate::objects::create_on_map(&mut s.objects, kind, 0, 1, x, y, z);
        crate::map::view::update_view(s);
        s.ui.input.selection = o;
        s.ui.input.target = (x as i16, y as i16);
    });
    let e = dispatch_chosen(&files, 0x15, setup, "^");
    let tail = show("Telekinesis on an object two east, north", &e);
    let s = e.state();
    let o = s.ui.input.selection;
    eprintln!(
        "  object {o} type {:#x} now at {:?}",
        s.objects.kind(o),
        at(&s, o)
    );
    assert!(at(&s, o) == (c.0 + 2, c.1 - 1), "one tile north: {tail:?}");
    assert!(
        tail.has(&text_at(e.io(), super::items::MSG_DIRECTION)),
        "{tail:?}"
    );
}

/// Gate Travel (0x64) with "1": the party goes to moonstone 0 (the start state's table).
#[test]
fn gate_travel_to_a_moonstone() {
    let Some(files) = files() else { return };
    let e = cast(&files, 0x64, Box::new(|_| {}), "1\n");
    let tail = show("Gate Travel to phase 1", &e);
    frame(&e, "gate-travel");
    let s = e.state();
    let stone = (
        s.actors.tail_word(0x2a),
        s.actors.tail_word(0x2c),
        s.actors.tail_word(0x2e),
    );
    let c = s.map.centre;
    eprintln!(
        "  moonstone 0 {stone:?}, centre now {c:?}, avatar {:?}",
        s.objects.location[1]
    );
    assert_eq!((c.x, c.y, u16::from(c.z)), stone, "{tail:?}");
    assert!(
        tail.has(&text_at(e.io(), super::travel::MSG_TO_PHASE)),
        "{tail:?}"
    );
}

/// Wizard Eye (0x69): three steps east with the arrows, the frame taken while the eye is out
/// (the view centre moved, every view tile shown), then escape: the centre comes back and the
/// eye is gone, `Msg::Done`.
#[test]
fn wizard_eye_moves_the_view() {
    let Some(files) = files() else { return };
    let c0 = start_centre(&files);
    let mut e = start(&files, 0x69, Box::new(|_| {}), ">>>");
    for _ in 0..8 {
        e.tick(&[], 16);
    }
    {
        let s = e.state();
        let c = s.map.centre;
        let eyes: Vec<usize> = (0x100..crate::objects::OBJECT_SLOTS)
            .filter(|&o| s.objects.type_frame[o] == 0x34)
            .collect();
        eprintln!(
            "  out: centre {c:?}, reveal_all {}, eye objects {eyes:?}",
            s.map.reveal_all
        );
        let east = (c0.0 + 3, c0.1);
        assert!((c.x, c.y) == east, "three steps east");
        assert_eq!(s.map.reveal_all, 1);
        assert_eq!(eyes.len(), 1);
        assert!(at(&s, eyes[0] as Slot) == east, "the eye at the centre");
    }
    frame(&e, "wizard-eye-out");
    for k in keys("~") {
        e.tick(&[k], 16);
    }
    finish(&mut e);
    let tail = show("Wizard Eye: east ×3, escape", &e);
    frame(&e, "wizard-eye-back");
    let s = e.state();
    assert!(
        (s.map.centre.x, s.map.centre.y) == (c0.0, c0.1),
        "the centre back on the caster"
    );
    assert_eq!(s.map.reveal_all, 0);
    assert!((0x100..crate::objects::OBJECT_SLOTS).all(|o| s.objects.type_frame[o] != 0x34));
    assert!(tail.has(&msg(&e, Msg::Done)), "{tail:?}");
}

/// Tremor (0x79): actor 7 made evil (alignment 0x20) is hurt, the others not evil are not.
#[test]
fn tremor_hurts_evil_actors() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        s.actors.attrs.status[7] = (s.actors.attrs.status[7] & 0x9f) | 0x20;
        s.actors.attrs.hp[7] = 60;
    });
    let before = State::new_game(&files).unwrap().actors.attrs.hp;
    let e = cast(&files, 0x79, setup, "");
    let tail = show("Tremor", &e);
    let s = e.state();
    eprintln!("  actor 7 hp 60 -> {}", s.actors.attrs.hp[7]);
    assert!((30..60).contains(&s.actors.attrs.hp[7]), "1..30 damage");
    let start = State::new_game(&files).unwrap();
    let others: Vec<usize> = (1..0xe0)
        .filter(|&a| a != 7 && start.objects.type_frame[a] != 0 && !evil(&start, a))
        .collect();
    assert!(!others.is_empty());
    for a in others {
        assert!(s.actors.attrs.hp[a] == before[a], "actor {a} is not evil");
    }
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Locate (0x35) at the start: latitude (y − 0x168) / 8 (north when not positive), longitude
/// (x − 0x130) / 8 (west when not positive), the degrees 0x11a470 prints.
#[test]
fn locate_prints_the_degrees() {
    let Some(files) = files() else { return };
    let e = cast(&files, 0x35, Box::new(|_| {}), "");
    let tail = show("Locate", &e);
    let c = e.state().map.centre;
    let lat = (c.y as i16 - 0x168) >> 3;
    let long = (c.x as i16 - 0x130) >> 3;
    // The formats and hemisphere letters 0x11a470 prints, from the user's `game`.
    use super::travel::{FMT_LAT, FMT_LONG, MSG_EAST, MSG_NORTH, MSG_SOUTH, MSG_WEST};
    let io = e.io();
    let ns = text_at(io, if lat > 0 { MSG_SOUTH } else { MSG_NORTH });
    let ew = text_at(io, if long > 0 { MSG_EAST } else { MSG_WEST });
    let want = printed(io, FMT_LAT, &[&lat.abs().to_string()])
        + &ns
        + &printed(io, FMT_LONG, &[&long.abs().to_string()])
        + &ew;
    eprintln!("  want {} characters, degrees ({lat}, {long})", want.len());
    assert!(tail.has(&want), "{tail:?}");
}

/// Armageddon (0x70): every actor slot below 0xff but the Avatar and actor 5 is gone,
/// the party is the Avatar alone and the eggs stop.
#[test]
fn armageddon_leaves_two() {
    let Some(files) = files() else { return };
    let before = State::new_game(&files).unwrap();
    let alive_before = (0..0xffusize)
        .filter(|&a| before.objects.type_frame[a] != 0)
        .count();
    let e = cast(&files, 0x70, Box::new(|_| {}), "");
    show("Armageddon", &e);
    frame(&e, "armageddon");
    let s = e.state();
    let left: Vec<usize> = (0..0xff)
        .filter(|&a| s.objects.type_frame[a] != 0)
        .collect();
    eprintln!("  actor slots in use: {alive_before} before, {left:?} after");
    assert_eq!(left, [1, 5]);
    assert_eq!(s.actors.party.size, 1);
    assert_eq!(s.map.flag_7afb, 1);
}

/// Energy Wind (0x62) north from the Avatar to four tiles up (cross-hair four up): actors 6 and
/// 7 put on the way and actor 5 at the end are hit; the other members, put behind the caster,
/// are not.
#[test]
fn energy_wind_hurts_along_its_path() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        place_at(s, 6, 0, -2);
        place_at(s, 7, 0, -3);
        place_at(s, 5, 0, -4);
        let behind: Vec<Slot> = s
            .actors
            .party
            .active()
            .skip(1)
            .map(|m| m.0 as Slot)
            .collect();
        for (i, m) in behind.into_iter().enumerate() {
            place_at(s, m, i as i16 - 1, 1);
        }
        for a in [5usize, 6, 7] {
            s.actors.attrs.hp[a] = 100;
        }
        crate::map::view::update_view(s);
    });
    let before = State::new_game(&files).unwrap();
    let e = cast(&files, 0x62, setup, "^^^^\n");
    let tail = show("Energy Wind four up", &e);
    frame(&e, "energy-wind");
    let s = e.state();
    let hp: Vec<(usize, u8)> = (2..8).map(|a| (a, s.actors.attrs.hp[a])).collect();
    eprintln!("  hit points after: {hp:?}");
    for a in [6usize, 7] {
        assert!(
            (70..100).contains(&s.actors.attrs.hp[a]),
            "actor {a} on the path"
        );
    }
    // Actor 5 at the end: hit, or its hit points 0xff when it is creature 0x1f, whom 0x1231be
    // never hurts.
    let end = s.actors.attrs.hp[5];
    let hit = if unhurt_kind(&s, 5) {
        end == 0xff
    } else {
        (70..100).contains(&end)
    };
    assert!(hit, "actor 5 at the end");
    for m in before.actors.party.active().skip(1) {
        let a = m.index();
        assert!(
            s.actors.attrs.hp[a] == before.actors.attrs.hp[a],
            "member {a} behind the caster"
        );
    }
    assert!(!tail.has(&msg(&e, Msg::NoEffect)), "{tail:?}");
}

/// Slime (0x76): actor 7 made evil becomes a slime (0x177); the others not evil keep their types.
#[test]
fn slime_turns_evil_actors() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        s.actors.attrs.status[7] = (s.actors.attrs.status[7] & 0x9f) | 0x20;
        dull(s, 7);
    });
    let start = State::new_game(&files).unwrap();
    let before = start.objects.type_frame.clone();
    let e = cast(&files, 0x76, setup, "");
    show("Slime", &e);
    let s = e.state();
    assert_eq!(s.objects.type_frame[7], 0x177);
    let others: Vec<usize> = (1..0xe0)
        .filter(|&a| a != 7 && before[a] != 0 && !evil(&start, a))
        .collect();
    assert!(!others.is_empty());
    for a in others {
        assert!(
            s.objects.type_frame[a] & 0x3ff == before[a] & 0x3ff,
            "actor {a} keeps its type"
        );
    }
}

/// Insect Swarm (0x42): eight swarms (0x157) on the caster's side near the centre.
#[test]
fn insect_swarm_makes_eight() {
    let Some(files) = files() else { return };
    let c = start_centre(&files);
    let e = cast(&files, 0x42, Box::new(|_| {}), "");
    let tail = show("Insect Swarm", &e);
    let s = e.state();
    let swarms: Vec<usize> = (0xe0..0x100)
        .filter(|&a| s.objects.type_frame[a] & 0x3ff == 0x157)
        .collect();
    eprintln!("  swarms in slots {swarms:x?}");
    assert!(!swarms.is_empty() && swarms.len() <= 8);
    for &a in &swarms {
        let (x, y) = at(&s, a as Slot);
        assert!(within((x, y), c, 5), "a swarm within 5 of the caster");
        assert_eq!(s.actors.attrs.status[a] & 0x60, 0x40);
    }
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Web (0x59) three tiles west of the Avatar: webs (0x35) on the free cells of the 3 × 3 there
/// (no actor in it: 0x11b1c0 tested with an object slot next to an actor panics in the actors
/// port, reported).
#[test]
fn web_covers_the_square() {
    let Some(files) = files() else { return };
    let c = start_centre(&files);
    let e = cast(&files, 0x59, Box::new(|_| {}), "<<<\n");
    let tail = show("Web three left", &e);
    frame(&e, "web");
    let mut s = e.state().clone();
    let mut webs = Vec::new();
    let (wx, wy) = (c.0 - 3, c.1);
    for x in wx - 1..=wx + 1 {
        for y in wy - 1..=wy + 1 {
            let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x, y, c.2);
            while o >= 0 {
                if s.objects.kind(o) == super::area::WEB {
                    webs.push((x, y));
                }
                o = crate::objects::next_at(&mut s.objects, &s.tables);
            }
        }
    }
    eprintln!("  webs at {webs:?}");
    assert!(
        webs.contains(&(wx, wy)),
        "a web on the aimed tile: {tail:?}"
    );
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Wing Strike (0x68): actor 7 made evil is struck once by the dragon that crosses its row; the
/// dragon is gone afterwards.
#[test]
fn wing_strike_hits_a_foe() {
    let Some(files) = files() else { return };
    let c = start_centre(&files);
    let setup: Setup = Box::new(|s| {
        s.actors.attrs.status[7] = (s.actors.attrs.status[7] & 0x9f) | 0x20;
        s.actors.attrs.hp[7] = 60;
    });
    let e = cast(&files, 0x68, setup, "");
    let tail = show("Wing Strike", &e);
    frame(&e, "wing-strike");
    let s = e.state();
    eprintln!("  actor 7 hp 60 -> {}", s.actors.attrs.hp[7]);
    assert!((40..60).contains(&s.actors.attrs.hp[7]), "1..20 damage");
    // Dragons of the world (0x19b) lie elsewhere in the loaded region; none is left in view.
    let left: Vec<(usize, (u16, u16))> = (0..crate::objects::OBJECT_SLOTS)
        .filter(|&o| s.objects.type_frame[o] & 0x3ff == 0x19b)
        .map(|o| (o, at(&s, o as Slot)))
        .filter(|&(_, t)| within(t, c, 6))
        .collect();
    eprintln!("  0x19b objects in view after: {left:?}");
    assert!(left.is_empty(), "the dragon is gone");
    let none = text_at(e.io(), super::creatures::MSG_NO_FOES);
    assert!(!none.is_empty() && !tail.has(&none), "{tail:?}");
}

/// Wing Strike with no foe in view: the no-foes line (0x115a66).
#[test]
fn wing_strike_without_foes() {
    let Some(files) = files() else { return };
    let e = cast(&files, 0x68, Box::new(|_| {}), "");
    let tail = show("Wing Strike, no foes", &e);
    let none = text_at(e.io(), super::creatures::MSG_NO_FOES);
    assert!(tail.has(&none), "{tail:?}");
}

/// Hail Storm (0x54) on actor 5, put four tiles north: random 10..30 stones; the hits take hit
/// points.
#[test]
fn hail_storm_falls() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        place_at(s, 5, 0, -4);
        s.actors.attrs.hp[5] = 200;
        crate::map::view::update_view(s);
    });
    let e = cast(&files, 0x54, setup, "^^^^\n");
    let tail = show("Hail Storm four up", &e);
    let s = e.state();
    eprintln!("  actor 5 hp 200 -> {}", s.actors.attrs.hp[5]);
    // A stone hit it: hit points lost, or 0xff when it is creature 0x1f (0x1231be).
    let hp = s.actors.attrs.hp[5];
    let hit = if unhurt_kind(&s, 5) {
        hp == 0xff
    } else {
        hp < 200
    };
    assert!(hit, "a stone hit actor 5");
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Vanish (0x19) on an object, then Reappear (0x13) at another place: it comes back there.
#[test]
fn reappear_brings_back_what_vanished() {
    let Some(files) = files() else { return };
    let c = start_centre(&files);
    let setup: Setup = Box::new(|s| {
        let kind = (1..0x400u16)
            .find(|&k| {
                let base = s.tables.basetile.base[usize::from(k)];
                s.tables.tileflags.flags3[usize::from(base)] & 8 != 0
            })
            .unwrap();
        let (x, y) = off(s, 2, 0);
        let z = u16::from(s.map.centre.z);
        let o = crate::objects::create_on_map(&mut s.objects, kind, 0, 1, x, y, z);
        crate::map::view::update_view(s);
        s.ui.input.selection = o;
        s.ui.input.target = (x as i16, y as i16);
    });
    let e = dispatch_chosen(&files, 0x19, setup, "");
    show("Vanish", &e);
    let tf = e.state().actors.tail_word(0x22);
    eprintln!("  vanished type {tf:#x}");
    assert_ne!(tf, 0);
    let after = e.state().clone();
    let setup: Setup = Box::new(move |s| {
        *s = after;
        s.ui.input.selection = -1;
        s.ui.input.target = (c.0 as i16 - 1, c.1 as i16 - 1);
    });
    let e = dispatch_chosen(&files, 0x13, setup, "");
    let tail = show("Reappear one left, one up", &e);
    let mut s = e.state().clone();
    let mut back = false;
    let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, c.0 - 1, c.1 - 1, c.2);
    while o >= 0 {
        back |= s.objects.type_frame[o as usize] == tf;
        o = crate::objects::next_at(&mut s.objects, &s.tables);
    }
    assert!(back, "{tail:?}");
    assert_eq!(s.actors.tail_word(0x22), 0, "forgotten");
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

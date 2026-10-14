//! Real data: the spells ported in the second magic unit, cast from the start state of the
//! user's archive.org zip — through the book with the incantation typed (its letters read from
//! the user's `game`) and the cross-hair moved by arrow keys where it can aim, and with the
//! target set as 0x1225c0 would return it where it cannot (objects). Compares the message
//! scroll with strings read from the user's `game` (printing only its line count) and asserts
//! the state changes; the visual ones write frames to /tmp/claude-501/. Skips where the zip is
//! missing; no game data in the repo.
//!
//! The start state: the Avatar (1) at the view centre (307, 352, 0) with members 2..4 just
//! south, actor 5 (creature 0x1f) four tiles north, actors 6 and 7 near it.
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
    for _ in 0..4000 {
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

/// Mass Sleep (0x26) on a group of three around (309, 349), aimed with the cross-hair two
/// right and three up.
#[test]
fn mass_sleep_on_a_group() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        place(s, 6, 310, 349);
        place(s, 8, 309, 350);
        for a in [6, 7, 8] {
            dull(s, a);
        }
        crate::map::view::update_view(s);
    });
    let e = cast(&files, 0x26, setup, ">>^^^\n");
    let tail = show("Mass Sleep at (309, 349)", &e);
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
    let e = cast(&files, 0x33, Box::new(|_| {}), "<^\n");
    let tail = show("Fire Field at (306, 351)", &e);
    frame(&e, "fire-field");
    let field = {
        let mut s = e.state().clone();
        let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, 306, 351, 0);
        while o >= 0 && s.objects.kind(o) != super::area::FIRE_FIELD {
            o = crate::objects::next_at(&mut s.objects, &s.tables);
        }
        o
    };
    eprintln!("  field object {field}");
    assert!(field >= 0x100, "a fire field at (306, 351): {tail:?}");
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
    let before = e.state().clone();
    let setup: Setup = Box::new(move |s| {
        *s = before;
        s.ui.input.selection = field;
        s.ui.input.target = (306, 351);
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
    assert!(x.abs_diff(307) <= 5 && y.abs_diff(352) <= 5);
    assert_eq!(s.actors.attrs.status[a] & 0x60, 0x40);
    assert_eq!(s.actors.worktype[a], 8);
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Telekinesis (0x15) on an object two tiles east of the Avatar (picked the way 0x1225c0
/// would), pushed one tile north with the up arrow.
#[test]
fn telekinesis_pushes_an_object() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        // The first type with a weight byte that is not a crank or a lever.
        let kind = (1..0x100u16)
            .find(|&k| s.tables.tileflags.weight[usize::from(k)] != 0 && k != 0x120 && k != 0x10c)
            .unwrap();
        let o = crate::objects::create_on_map(&mut s.objects, kind, 0, 1, 309, 352, 0);
        crate::map::view::update_view(s);
        s.ui.input.selection = o;
        s.ui.input.target = (309, 352);
    });
    let e = dispatch_chosen(&files, 0x15, setup, "^");
    let tail = show("Telekinesis on an object at (309, 352), north", &e);
    let s = e.state();
    let o = s.ui.input.selection;
    eprintln!(
        "  object {o} type {:#x} now at {:?}",
        s.objects.kind(o),
        at(&s, o)
    );
    assert_eq!(at(&s, o), (309, 351), "{tail:?}");
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
    let mut e = start(&files, 0x69, Box::new(|_| {}), ">>>");
    for _ in 0..1000 {
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
        assert_eq!((c.x, c.y), (310, 352), "three steps east");
        assert_eq!(s.map.reveal_all, 1);
        assert_eq!(eyes.len(), 1);
        assert_eq!(at(&s, eyes[0] as Slot), (310, 352));
    }
    frame(&e, "wizard-eye-out");
    for k in keys("~") {
        e.tick(&[k], 16);
    }
    finish(&mut e);
    let tail = show("Wizard Eye: east ×3, escape", &e);
    frame(&e, "wizard-eye-back");
    let s = e.state();
    assert_eq!((s.map.centre.x, s.map.centre.y), (307, 352));
    assert_eq!(s.map.reveal_all, 0);
    assert!((0x100..crate::objects::OBJECT_SLOTS).all(|o| s.objects.type_frame[o] != 0x34));
    assert!(tail.has(&msg(&e, Msg::Done)), "{tail:?}");
}

/// Tremor (0x79): the evil actor 7 (alignment 0x20) is hurt, the good ones are not.
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
    for a in [2usize, 3, 4, 5, 6] {
        assert_eq!(s.actors.attrs.hp[a], before[a], "actor {a} is not evil");
    }
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Locate (0x35) at (307, 352, 0): latitude (352 − 360) / 8 = −1 (north), longitude
/// (307 − 304) / 8 = 0 (west).
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

/// Energy Wind (0x62) north from the Avatar at (307, 348) (cross-hair four up): actors 6 and 7
/// on the way and actor 5 at the end are hurt; the members behind the caster are not.
#[test]
fn energy_wind_hurts_along_its_path() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        place(s, 6, 307, 350);
        place(s, 7, 307, 349);
        for a in [5usize, 6, 7] {
            s.actors.attrs.hp[a] = 100;
        }
        crate::map::view::update_view(s);
    });
    let before = State::new_game(&files).unwrap().actors.attrs.hp;
    let e = cast(&files, 0x62, setup, "^^^^\n");
    let tail = show("Energy Wind to (307, 348)", &e);
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
    // Actor 5 is creature 0x1f, whom 0x1231be never hurts: its hit points become 0xff.
    assert_eq!(s.actors.attrs.hp[5], 0xff, "actor 5 at the end");
    for a in [2usize, 3, 4] {
        assert_eq!(
            s.actors.attrs.hp[a], before[a],
            "member {a} behind the caster"
        );
    }
    assert!(!tail.has(&msg(&e, Msg::NoEffect)), "{tail:?}");
}

/// Slime (0x76): the evil actor 7 becomes a slime (0x177); the good ones keep their types.
#[test]
fn slime_turns_evil_actors() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        s.actors.attrs.status[7] = (s.actors.attrs.status[7] & 0x9f) | 0x20;
        dull(s, 7);
    });
    let before = State::new_game(&files).unwrap().objects.type_frame;
    let e = cast(&files, 0x76, setup, "");
    show("Slime", &e);
    let s = e.state();
    assert_eq!(s.objects.type_frame[7], 0x177);
    for a in [1usize, 2, 3, 4, 5, 6] {
        assert_eq!(
            s.objects.type_frame[a] & 0x3ff,
            before[a] & 0x3ff,
            "actor {a}"
        );
    }
}

/// Insect Swarm (0x42): eight swarms (0x157) on the caster's side near the centre.
#[test]
fn insect_swarm_makes_eight() {
    let Some(files) = files() else { return };
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
        assert!(x.abs_diff(307) <= 5 && y.abs_diff(352) <= 5);
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
    let e = cast(&files, 0x59, Box::new(|_| {}), "<<<\n");
    let tail = show("Web at (304, 352)", &e);
    frame(&e, "web");
    let mut s = e.state().clone();
    let mut webs = Vec::new();
    for x in 303..=305 {
        for y in 351..=353 {
            let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x, y, 0);
            while o >= 0 {
                if s.objects.kind(o) == super::area::WEB {
                    webs.push((x, y));
                }
                o = crate::objects::next_at(&mut s.objects, &s.tables);
            }
        }
    }
    eprintln!("  webs at {webs:?}");
    assert!(webs.contains(&(304, 352)), "{tail:?}");
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Wing Strike (0x68): the evil actor 7 is struck once by the dragon that crosses its row; the
/// dragon is gone afterwards.
#[test]
fn wing_strike_hits_a_foe() {
    let Some(files) = files() else { return };
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
        .filter(|&(_, (x, y))| x.abs_diff(307) <= 6 && y.abs_diff(352) <= 6)
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

/// Hail Storm (0x54) on actor 5's tile: random 10..30 stones; the hits take hit points.
#[test]
fn hail_storm_falls() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| s.actors.attrs.hp[5] = 200);
    let e = cast(&files, 0x54, setup, "^^^^\n");
    let tail = show("Hail Storm at (307, 348)", &e);
    let s = e.state();
    eprintln!("  actor 5 hp 200 -> {}", s.actors.attrs.hp[5]);
    // A stone hit it: 0x1231be leaves creature 0x1f (actor 5) at 0xff hit points.
    assert_eq!(s.actors.attrs.hp[5], 0xff);
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

/// Vanish (0x19) on an object, then Reappear (0x13) at another place: it comes back there.
#[test]
fn reappear_brings_back_what_vanished() {
    let Some(files) = files() else { return };
    let setup: Setup = Box::new(|s| {
        let kind = (1..0x400u16)
            .find(|&k| {
                let base = s.tables.basetile.base[usize::from(k)];
                s.tables.tileflags.flags3[usize::from(base)] & 8 != 0
            })
            .unwrap();
        let o = crate::objects::create_on_map(&mut s.objects, kind, 0, 1, 309, 352, 0);
        crate::map::view::update_view(s);
        s.ui.input.selection = o;
        s.ui.input.target = (309, 352);
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
        s.ui.input.target = (306, 351);
    });
    let e = dispatch_chosen(&files, 0x13, setup, "");
    let tail = show("Reappear at (306, 351)", &e);
    let mut s = e.state().clone();
    let mut back = false;
    let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, 306, 351, 0);
    while o >= 0 {
        back |= s.objects.type_frame[o as usize] == tf;
        o = crate::objects::next_at(&mut s.objects, &s.tables);
    }
    assert!(back, "{tail:?}");
    assert_eq!(s.actors.tail_word(0x22), 0, "forgotten");
    assert!(tail.has(&msg(&e, Msg::Success)), "{tail:?}");
}

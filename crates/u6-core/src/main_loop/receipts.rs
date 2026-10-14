//! Integration receipts (M2) on the user's zip through the real command loop: the boot of
//! [`super::run`] up to the prompt (the start egg left unhatched, out of combat), a state set up
//! where keys from the boot cannot reach (no spellbook at the start; the castle is not
//! wilderness), then [`super::commands::command_loop`] fed one key per tick. Cast reaches magic
//! 0x11af10 and Rest 0x127b1a through the loop's dispatch. With `U6_RECEIPTS=<dir>` the frames
//! go there as raw RGBA (320 × 200). Skips where the zip is missing; reads it, copies nothing.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::cell::RefCell;
use std::path::Path;

use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::objects::{IN_CONTAINER, READIED};
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
const AVATAR: i16 = 1;

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data: std::fs::read(ZIP).unwrap(),
    }]);
    Some(Files::new(r.files))
}

type Setup = Box<dyn FnOnce(&mut State)>;

/// [`super::run`] with the start egg unhatched (A4-0x7afb around the region load), out of
/// combat, and `setup` before the command loop.
async fn boot_then(state: &RefCell<State>, io: &Io, setup: Setup) {
    super::boot_screen(state, io);
    {
        let mut s = state.borrow_mut();
        s.main.flag_64ce = 1;
        s.map.flag_7afb = 1;
        super::finish_load_state(&mut s, io);
        s.map.flag_7afb = 0;
        let anim = s.tables.anim.clone();
        for slot in anim.slots.iter().take(usize::from(anim.count)) {
            crate::gfx::anim::substitute(&mut s.gfx, slot.tile_to_animate, slot.first_anim_frame);
        }
        s.actors.objlist_tail[super::TAIL_CREATED] = 1;
        crate::actors::turn::after_load(&mut s);
        crate::actors::clock::tick(&mut s, 0);
        crate::combat::mode::break_off(&mut s);
        setup(&mut s);
        crate::map::view::update_view(&mut s);
        s.ui.panel.dirty = true;
    }
    super::flush_prints(state, io).await;
    super::commands::command_loop(state, io).await;
}

fn engine(files: &Files, setup: Setup) -> Engine {
    let state = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
    let mut e = Engine::with_game(state, files.clone(), move |st, io| async move {
        boot_then(&st, &io, setup).await;
    });
    idle(&mut e, 30);
    e
}

fn idle(e: &mut Engine, n: u32) {
    for _ in 0..n {
        e.tick(&[], 16);
    }
}

fn press(e: &mut Engine, keys: &[Key], settle: u32) {
    for &k in keys {
        e.tick(&[InputEvent::Key(k)], 16);
        idle(e, settle);
    }
}

fn chars(text: &str) -> Vec<Key> {
    text.chars()
        .map(|c| match c {
            '\n' => Key::Enter,
            '>' => Key::Right,
            c => Key::Char(c),
        })
        .collect()
}

fn dump(e: &Engine, name: &str) {
    if let Ok(dir) = std::env::var("U6_RECEIPTS") {
        let mut rgba = Vec::new();
        e.frame().write_rgba(&mut rgba);
        std::fs::write(Path::new(&dir).join(format!("{name}.rgba")), rgba).unwrap();
    }
}

fn scroll(e: &Engine, from: &str) -> Vec<String> {
    let lines = e.state().ui.messages.clone();
    let at = lines.iter().rposition(|l| l.contains(from)).unwrap_or(0);
    lines[at..].to_vec()
}

/// A readied spellbook holding `spells` for the Avatar and 10 of each reagent.
fn give_book(s: &mut State, spells: &[u8]) {
    let book = crate::objects::create_in(s, crate::magic::SPELLBOOK, READIED, 0, AVATAR);
    for &n in spells {
        crate::objects::create_in(
            s,
            crate::magic::SPELL,
            IN_CONTAINER,
            u16::from(n) << 8,
            book,
        );
    }
    for kind in s.magic.tables.reagents {
        crate::objects::add_to_owner(s, AVATAR, kind, 10);
    }
}

fn lit_cells(s: &State) -> usize {
    (0..11)
        .flat_map(|r| (0..11).map(move |c| (c, r)))
        .filter(|&(c, r)| {
            let t = s.map.view_tile(c, r);
            t != crate::map::view::NOT_VISIBLE && t != crate::map::view::DARK
        })
        .count()
}

/// C at 22:00 with a book holding Light (9): the book panel, then "In Lor" lights the view.
#[test]
fn cast_light_at_night_through_the_command_loop() {
    let Some(files) = files() else { return };
    let mut e = engine(
        &files,
        Box::new(|s| {
            give_book(s, &[0, 1, 6, 9, 0x11, 0x14]);
            s.actors.calendar.hour = 22;
            crate::actors::clock::tick(s, 0);
        }),
    );
    let dark = lit_cells(&e.state());
    press(&mut e, &chars("c"), 10);
    let book = e.state().ui.panel.mode;
    dump(&e, "int-b-light-book");
    press(&mut e, &chars("il\n"), 4);
    idle(&mut e, 40);
    dump(&e, "int-b-light-lit");
    let s = e.state();
    let lit = lit_cells(&s);
    let tail = scroll(&e, "Cast-");
    eprintln!("light: panel {book:#x} after C; lit cells {dark} -> {lit}; {tail:?}");
    assert_eq!(book, crate::magic::BOOK_PANEL, "the spellbook panel is up");
    assert!(tail.iter().any(|l| l.contains("(Light)")), "{tail:?}");
    assert_eq!(s.actors.timers[0], 100, "the light timer runs");
    assert!(lit > dark, "{lit} lit cells after, {dark} before");
}

/// C, "Ort Jux" (Magic Arrow) at a gargoyle three tiles east: the flight is shown (a frame
/// between the cast and the hit), the gargoyle hurt or the dodge reported.
#[test]
fn magic_arrow_at_a_gargoyle_through_the_command_loop() {
    let Some(files) = files() else { return };
    let gargoyle = std::rc::Rc::new(std::cell::Cell::new(-1i16));
    let g = std::rc::Rc::clone(&gargoyle);
    let mut e = engine(
        &files,
        Box::new(move |s| {
            give_book(s, &[0x11]);
            let kind = [0x16au16, 0x16b]
                .into_iter()
                .find(|&k| crate::objects::monster_index(&s.objects, k) >= 0)
                .expect("a gargoyle type");
            let c = s.map.centre;
            g.set(crate::objects::commands::spawn_monster(
                s,
                kind,
                c.x + 3,
                c.y,
                u16::from(c.z),
            ));
        }),
    );
    let foe = gargoyle.get();
    assert!(foe >= 0, "the gargoyle was made");
    let hp0 = e.state().actors.attrs.hp[foe as usize];
    press(&mut e, &chars("c"), 10);
    press(&mut e, &chars("oj\n"), 4);
    press(&mut e, &chars(">>>"), 2);
    let pixels = |e: &Engine| {
        let mut v = Vec::new();
        e.frame().write_rgba(&mut v);
        v
    };
    let before = pixels(&e);
    let shown0 = e.state().combat.shown;
    e.tick(&[InputEvent::Key(Key::Enter)], 16);
    let mut flight = None;
    let mut last = shown0;
    for t in 0..200 {
        let shown = e.state().combat.shown;
        if shown > last && pixels(&e) != before {
            flight.get_or_insert(t);
            if shown - shown0 <= 12 {
                dump(&e, &format!("int-b-arrow-flight-{:02}", shown - shown0));
            }
        }
        last = shown;
        e.tick(&[], 16);
    }
    idle(&mut e, 40);
    dump(&e, "int-b-arrow-after");
    let s = e.state();
    let tail = scroll(&e, "Cast-");
    let hp1 = s.actors.attrs.hp[foe as usize];
    eprintln!(
        "arrow: frames shown {} (first new one at tick {flight:?}); gargoyle hp {hp0} -> {hp1}; {tail:?}",
        s.combat.shown - shown0
    );
    assert!(tail.iter().any(|l| l.contains("(Magic Arrow)")), "{tail:?}");
    assert!(flight.is_some(), "a flight frame was shown");
    assert!(
        hp1 < hp0 || tail.iter().any(|l| l.contains("Failed")),
        "{tail:?}"
    );
}

/// B is not needed (out of combat); the party is put on the first spot east of the castle where
/// every camp check of 0x127b1a passes, then R, 1 hour, Dupre (2) on guard: the camp lines.
#[test]
fn rest_one_hour_through_the_command_loop() {
    let Some(files) = files() else { return };
    let spot = std::rc::Rc::new(std::cell::Cell::new(None));
    let sp = std::rc::Rc::clone(&spot);
    let mut e = engine(
        &files,
        Box::new(move |s| {
            let c = s.map.centre;
            'search: for d in (16..120u16).step_by(4) {
                for (x, y) in [(c.x + d, c.y), (c.x, c.y + d), (c.x - d, c.y)] {
                    s.map.flag_7afb = 1;
                    crate::map::travel::teleport(s, x, y, 0, 0);
                    s.map.flag_7afb = 0;
                    if crate::actors::rest::refusal(s).is_none() {
                        sp.set(Some((x, y)));
                        break 'search;
                    }
                }
            }
            crate::objects::build_draw_lists(s);
        }),
    );
    let at = spot.get().expect("a camp spot near the castle");
    let hour0 = e.state().actors.calendar.hour;
    dump(&e, "int-b-rest-before");
    press(&mut e, &chars("r"), 10);
    press(&mut e, &chars("1"), 10);
    press(&mut e, &chars("2"), 10);
    let mut camp = None;
    for t in 0..6000 {
        e.tick(&[], 16);
        if camp.is_none() && e.state().ui.messages.iter().any(|l| l.contains("guard")) {
            camp = Some(t);
        }
        if t == 400 {
            dump(&e, "int-b-rest-camp");
        }
        let s = e.state();
        if s.actors.calendar.hour != hour0
            && s.ui.input.target_command == crate::ui::globals::NO_COMMAND
        {
            break;
        }
    }
    idle(&mut e, 60);
    dump(&e, "int-b-rest-after");
    let s = e.state();
    let tail = scroll(&e, "Rest");
    eprintln!(
        "rest at {at:?}: {:02}:{:02} -> {:02}:{:02}; {tail:?}",
        hour0, 0, s.actors.calendar.hour, s.actors.calendar.minute
    );
    assert!(tail.iter().any(|l| l.contains("guard")), "{tail:?}");
    assert_ne!(s.actors.calendar.hour, hour0, "an hour passed");
}

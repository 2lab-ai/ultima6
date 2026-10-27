//! Integration receipts (M2) on the user's zip through the real command loop: the boot of
//! [`super::run`] up to the prompt (the start egg left unhatched, out of combat), a state set up
//! where keys from the boot cannot reach (no spellbook at the start; the castle is not
//! wilderness), then [`super::commands::command_loop`] fed one key per tick. Cast reaches magic
//! 0x11af10 and Rest 0x127b1a through the loop's dispatch; the spellbook also by clicks (0x106474). With `U6_RECEIPTS=<dir>` the frames
//! go there as raw RGBA (the frame, `u6_render::WIDTH` × `HEIGHT`). Skips where the zip is missing; reads it, copies nothing.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::cell::RefCell;
use std::path::Path;

use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::geom::{VIEW_H, VIEW_W};
use crate::image::A4;
use crate::objects::{IN_CONTAINER, READIED};
use crate::state::State;
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
const HERO: i16 = 1;
/// Where the receipts read the lines they look for (the user's `game`): the command-name table
/// at A4-0x7c8e and the format its names are printed with (0x101f48), Cast's code, Rest's own
/// name line (0x1272cc) and the line after the guard's name (0x1273fc).
const COMMAND_NAMES: u32 = 0x7c8e;
const FMT_NAME_DASH: u32 = 0x0010_1f48;
const CAST: u32 = 0x82;
const STR_REST: u32 = 0x0012_72cc;
const STR_STANDS_GUARD: u32 = 0x0012_73fc;

pub(super) fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data: std::fs::read(ZIP).quiet(),
    }]);
    Some(Files::new(r.files))
}

pub(super) type Setup = Box<dyn FnOnce(&mut State)>;

/// [`super::run`] with the start egg unhatched (A4-0x7afb around the region load), out of
/// combat, and `setup` before the command loop.
pub(super) async fn boot_then(state: &RefCell<State>, io: &Io, setup: Setup) {
    super::boot_screen(state, io);
    {
        let mut s = state.borrow_mut();
        super::test_support::load_state_like_main(&mut s, io);
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
    let state = State::new_game(files).quiet();
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

pub(super) fn chars(text: &str) -> Vec<Key> {
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
        std::fs::write(Path::new(&dir).join(format!("{name}.rgba")), rgba).quiet();
    }
}

fn scroll(e: &Engine, from: &str) -> Vec<String> {
    let lines = e.state().ui.messages.clone();
    let at = lines.iter().rposition(|l| l.contains(from)).unwrap_or(0);
    lines[at..].to_vec()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The line the command loop prints for command `code`: its name through the name format.
fn command_line(e: &Engine, code: u32) -> String {
    let name = super::pointed_string(e.io(), A4.wrapping_sub(COMMAND_NAMES) + 4 * code);
    let line = text(&super::game_string(e.io(), FMT_NAME_DASH)).replace("%s", &text(&name));
    assert!(!name.is_empty(), "the name of command {code:#x}");
    line
}

/// The spell `n`'s name (the magic tables read from the user's `game`).
fn spell_name(e: &Engine, n: usize) -> String {
    text(&e.state().magic.tables.names[n])
}

/// Whether the scroll `lines` hold `want` with the word wrap undone (the lines joined, every run
/// of white space as one space).
fn holds(lines: &[String], want: &str) -> bool {
    let flat = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    let want = flat(want);
    !want.is_empty() && flat(&lines.join(" ")).contains(&want)
}

/// A readied spellbook holding `spells` for the hero and 10 of each reagent.
pub(super) fn give_book(s: &mut State, spells: &[u8]) {
    let book = crate::objects::create_in(s, crate::magic::SPELLBOOK, READIED, 0, HERO);
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
        crate::objects::add_to_owner(s, HERO, kind, 10);
    }
}

/// Every egg on the map marked hatched (status 0x40), so the egg pass of a window rebuild
/// (0x1321dc) only marks it again (0x1319b2) and a receipt's walk meets no hatchlings. The start
/// egg, which [`boot_then`] leaves unhatched, has status 0x20 and is offered at every rebuild;
/// the window rebuilds when the leader leaves window cells `LEAD..=LEAD + 7` (`geom::LEAD`: 16 in
/// Original, 26 in Wide), which in Wide the receipts' first steps from the start do.
pub(super) fn hatch_no_eggs(s: &mut State) {
    /// The egg type 0x1321dc compares with.
    const EGG: u16 = 0x14f;
    for o in 0x100..crate::objects::OBJECT_SLOTS as crate::objects::Slot {
        if s.objects.kind(o) == EGG && s.objects.place(o) == 0 {
            s.objects.obj_status[o as usize] |= 0x40;
        }
    }
}

/// View cells drawn lit (neither unseen nor dark), over the whole view: [`VIEW_H`] rows of
/// [`VIEW_W`].
fn lit_cells(s: &State) -> usize {
    (0..VIEW_H)
        .flat_map(|r| (0..VIEW_W).map(move |c| (c, r)))
        .filter(|&(c, r)| {
            let t = s.map.view_tile(c, r);
            t != crate::map::view::NOT_VISIBLE && t != crate::map::view::DARK
        })
        .count()
}

/// C at 22:00 with a book holding Light (9): the book panel, then its two words (keys I, L)
/// light the view.
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
    idle(&mut e, 1000);
    dump(&e, "int-b-light-lit");
    let s = e.state();
    let lit = lit_cells(&s);
    let tail = scroll(&e, &command_line(&e, CAST));
    assert!(
        book == crate::magic::BOOK_PANEL,
        "the spellbook panel is up"
    );
    let spell = spell_name(&e, 9);
    assert!(holds(&tail, &spell), "no spell name in the scroll");
    assert!(s.actors.timers[0] == 100, "the light timer runs");
    assert!(lit > dark, "more view cells lit after the cast");
}

/// C, Magic Arrow's two words (keys O, J) at a gargoyle three tiles east: the flight is shown
/// (a frame between the cast and the hit), the gargoyle hurt or the dodge reported.
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
    for t in 0..2000 {
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
    idle(&mut e, 1000);
    dump(&e, "int-b-arrow-after");
    let s = e.state();
    let tail = scroll(&e, &command_line(&e, CAST));
    let hp1 = s.actors.attrs.hp[foe as usize];
    let spell = spell_name(&e, 0x11);
    let failed = text(s.magic.tables.message(crate::magic::Msg::Failed));
    assert!(holds(&tail, &spell), "no spell name in the scroll");
    assert!(flight.is_some(), "a flight frame was shown");
    assert!(
        hp1 < hp0 || holds(&tail, &failed),
        "the gargoyle neither hurt nor the failure line"
    );
}

/// B is not needed (out of combat); the party is put on the first spot east of the castle where
/// every camp check of 0x127b1a passes, then R, 1 hour, the second member (2) on guard: the
/// camp lines.
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
    assert!(spot.get().is_some(), "a camp spot near the castle");
    let hour0 = e.state().actors.calendar.hour;
    dump(&e, "int-b-rest-before");
    press(&mut e, &chars("r"), 10);
    press(&mut e, &chars("1"), 10);
    press(&mut e, &chars("2"), 10);
    let guard = text(&super::game_string(e.io(), STR_STANDS_GUARD));
    let mut camp = None;
    for t in 0..6000 {
        e.tick(&[], 16);
        if camp.is_none() && holds(&e.state().ui.messages, &guard) {
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
    let tail = scroll(&e, &text(&super::game_string(e.io(), STR_REST)));
    assert!(holds(&tail, &guard), "no guard line in the scroll");
    assert!(s.actors.calendar.hour != hour0, "an hour passed");
}

/// A press and its release in one tick (the event wait samples both: one click), then `settle`
/// idle ticks.
fn click(e: &mut Engine, x: i16, y: i16, settle: u32) {
    e.tick(
        &[
            InputEvent::Pointer { x, y, buttons: 1 },
            InputEvent::Pointer { x, y, buttons: 0 },
        ],
        16,
    );
    idle(e, settle);
}

/// The spellbook by the pointer (0x106474) at 22:00: C opens the book on the first circle's
/// page, a click on its fourth tab pages to the next circle (0xa5), the first tab back (0xa2),
/// and a click on Light's line (the fourth, 8 pixels a line from y 0x10) casts it (0x8e with the
/// line selected): the view lights.
#[test]
fn the_spellbook_by_clicks_through_the_command_loop() {
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
    let page = |e: &Engine| {
        let m = &e.state().magic;
        (m.page[..usize::from(m.page_lines)].to_vec(), m.next_circle)
    };
    let first = page(&e);
    assert!(e.state().ui.panel.mode == crate::magic::BOOK_PANEL);
    dump(&e, "int-mouse-book");
    click(&mut e, 0xe8, 0x5c, 10);
    let next = page(&e);
    dump(&e, "int-mouse-book-next-circle");
    click(&mut e, 0xb8, 0x5c, 10);
    let back = page(&e);
    click(&mut e, 0xc8, 0x10 + 3 * 8 + 4, 4);
    idle(&mut e, 1000);
    dump(&e, "int-mouse-book-light");
    let s = e.state();
    let lit = lit_cells(&s);
    let tail = scroll(&e, &command_line(&e, CAST));
    assert!(first.0 == [0, 1, 6, 9], "the first circle's page");
    assert!(next.0 == [0x11, 0x14], "the next-circle tab");
    assert!(back.0 == first.0, "the previous-circle tab");
    let spell = spell_name(&e, 9);
    assert!(holds(&tail, &spell), "no spell name in the scroll");
    assert!(s.actors.timers[0] == 100, "the light timer runs");
    assert!(lit > dark, "more view cells lit after the cast");
}

/// The executable strings' counts of `s` (`crate::locale::LocaleState::texts`): from the pack,
/// misses, and (2026-10-27, spec 16 item 1) shown with no catalogue entry, in English whatever
/// the pack.
fn text_counts(s: &State) -> [u32; 3] {
    let t = &s.locale.texts;
    let [hits, misses] = t.get();
    [hits, misses, t.uncatalogued.get()]
}

/// Spec 16 item 1 (2026-10-27): [`text_counts`] reads the three counts of the executable strings
/// in the receipts' order. A Korean pack over a synthetic `game` of the test's own bytes: one
/// listed line it has, shown once (a hit), one it lacks, shown twice (two misses), one line no
/// list names, shown three times (three uncatalogued), so a permutation of the three fails.
#[test]
fn the_receipts_counts_hold_the_uncatalogued_count_beside_hits_and_misses() {
    let (listed, lacked) = (crate::map::seams::PRINTED[1], crate::map::seams::PRINTED[2]);
    let unlisted = crate::image::IMAGE_BASE + 0x100;
    let mut s = State::empty();
    crate::locale::strings::install_test_forms(
        &mut s,
        &[
            (listed, b"Zq1 lead"),
            (lacked, b"Zq4 other"),
            (unlisted, b"Zq2 line"),
        ],
        &[(b"Zq1 lead", "가")],
    );
    assert_eq!(text_counts(&s), [0, 0, 0], "nothing shown yet");
    let lines: [&[u8]; 6] = [
        b"Zq1 lead",
        b"Zq4 other",
        b"Zq4 other",
        b"Zq2 line",
        b"Zq2 line",
        b"Zq2 line",
    ];
    for line in lines {
        let _ = crate::locale::exe_string(&s, line);
    }
    assert_eq!(
        text_counts(&s),
        [1, 2, 3],
        "a hit, two misses, three uncatalogued"
    );
}

/// Spec 08 Order 3 / Acceptance 5 (slices K2 and K3), through the command loop: the same keys
/// with and without the identity pack of the owner's files (`crate::locale::identity`, installed
/// before the loop starts, `wide` on): Look, Talk and Get at the leader's own cell, Begin and
/// break off combat, a key the loop does not take, then Rest for an hour at a camp spot with the
/// second member on guard. Every tick shows the same frame and scroll as English, and every
/// catalogued string shown is one of the pack's (0 misses), and (2026-10-27) every executable
/// string shown is catalogued (0 uncatalogued); Look here is at the leader's own cell, where no
/// object name is shown. Prints counts and pass/fail only.
#[test]
fn the_identity_pack_shows_the_command_loop_exactly_as_english() {
    use crate::converse::disasm::catalog::hash64;
    let Some(files) = files() else { return };
    let pack = crate::locale::identity(&files).quiet();
    // A tick's frame and scroll, as hashes.
    let seen = |e: &Engine| {
        let s = e.state();
        let t = &s.ui.text;
        let mut text: Vec<u8> =
            s.ui.messages
                .iter()
                .flat_map(|l| l.bytes().chain([b'\n']))
                .collect();
        text.extend(t.line.iter().flat_map(|c| c.to_le_bytes()));
        (hash64(e.frame().pixels()), hash64(&text))
    };
    let play = |with: bool| {
        let (f, p) = (files.clone(), pack.clone());
        let setup: Setup = Box::new(move |s| {
            if with {
                crate::locale::install(s, &f, p, None).quiet();
            }
            let c = s.map.centre;
            'search: for d in (16..120u16).step_by(4) {
                for (x, y) in [(c.x + d, c.y), (c.x, c.y + d), (c.x - d, c.y)] {
                    s.map.flag_7afb = 1;
                    crate::map::travel::teleport(s, x, y, 0, 0);
                    s.map.flag_7afb = 0;
                    if crate::actors::rest::refusal(s).is_none() {
                        break 'search;
                    }
                }
            }
            crate::objects::build_draw_lists(s);
        });
        let mut e = engine(&files, setup);
        assert_eq!(e.state().ui.text.wide, with);
        let mut ticks = vec![seen(&e)];
        for key in chars("l\nt\ng\nbbqr12") {
            e.tick(&[InputEvent::Key(key)], 16);
            ticks.push(seen(&e));
            for _ in 0..12 {
                e.tick(&[], 16);
                ticks.push(seen(&e));
            }
        }
        let hour0 = e.state().actors.calendar.hour;
        for _ in 0..6000 {
            e.tick(&[], 16);
            ticks.push(seen(&e));
            let s = e.state();
            if s.actors.calendar.hour != hour0
                && s.ui.input.target_command == crate::ui::globals::NO_COMMAND
            {
                break;
            }
        }
        // Object-name coverage is the object-commands receipt's (names shown > 0, misses 0).
        let counts = text_counts(&e.state());
        (ticks, counts)
    };
    let (en, _) = play(false);
    let (id, [hits, misses, uncatalogued]) = play(true);
    let differ = en.iter().zip(&id).filter(|(a, b)| a != b).count();
    eprintln!(
        "identity pack, command loop: ticks {} / {}, differing {differ}; game strings from the pack {hits}, misses {misses}, uncatalogued {uncatalogued}",
        en.len(),
        id.len()
    );
    assert_eq!(en.len(), id.len(), "as many ticks");
    assert_eq!(differ, 0, "every frame and scroll as in English");
    assert!(hits > 0 && misses == 0);
    assert_eq!(
        uncatalogued, 0,
        "every executable string shown is catalogued"
    );
    eprintln!(
        "identity pack command loop identical to English: pass; misses 0: pass; uncatalogued 0: pass"
    );
}

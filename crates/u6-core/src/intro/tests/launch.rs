//! The launcher's sequence on the user's files (`Engine::with_seed`: `u`, then `game`, as
//! `main_loop::run` ports ultcli 0x100360). Skips when the zip is absent; reads only. The
//! receipt run (ignored by default) writes PNGs and raw indices to `U6_RECEIPT_DIR` (default:
//! the system temp directory), outside the repository.
#![allow(clippy::disallowed_methods)] // the tests read the zip and write receipt images

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::real::{Run, create_from_menu, files, png};
use crate::engine::Key;
use crate::files::Files;
use crate::main_loop::Program;
use crate::saveload::{MemStore, OBJLIST_FILE, SaveSync, attach, changed_since};

/// The start of "You must first create or transfer a character." (the scroll holds the lines
/// the window wrapped it into; the last one stays open while the cursor waits after it).
const NO_CHARACTER: &str = "You must first create or transfer a";

fn program(r: &Run) -> Program {
    r.e.state().main.program
}

/// The message scroll joined into one string (the message wraps over several lines).
fn said(r: &Run) -> String {
    r.e.state().ui.messages.join(" ")
}

/// The command loop waits for a command key with nothing queued.
fn at_prompt(r: &Run) -> bool {
    r.e.state().main.reading == 1 && r.e.io().pending_events() == 0
}

/// Ticks until the game's prompt is up (at most `max` ticks); true when it is.
fn to_prompt(r: &mut Run, max: u32) -> bool {
    for _ in 0..max {
        if at_prompt(r) {
            return true;
        }
        r.tick(1);
    }
    at_prompt(r)
}

fn leader_name(r: &Run) -> String {
    let s = r.e.state();
    s.actors.party.names[usize::from(s.actors.party.leader_index)].clone()
}

/// The opening skipped (Escape at its first frames), the menu up.
fn to_menu(r: &mut Run) {
    r.tick(30);
    r.keys(&[Key::Escape], 120);
}

#[test]
fn journey_onward_without_a_character_says_so_then_u_runs_again_and_creates_one() {
    let Some(files) = files() else { return };
    let mut r = Run::launcher(files, 0);
    r.tick(1);
    assert_eq!(program(&r), Program::U, "the launcher runs u first");
    to_menu(&mut r);
    r.keys(&[Key::Char('j')], 200);
    assert_eq!(program(&r), Program::Game, "Journey Onward starts game");
    assert!(said(&r).contains(NO_CHARACTER), "{:?}", said(&r));
    assert!(!r.e.finished());
    // A key: game exits with code 0, the launcher's state 0 runs u again from its opening.
    r.keys(&[Key::Space], 30);
    assert_eq!(program(&r), Program::U);
    assert!(!r.e.finished());
    // The second u makes a character; the second game (a new process) plays it.
    to_menu(&mut r);
    create_from_menu(&mut r, |_, _| {});
    assert_eq!(program(&r), Program::Game);
    assert!(to_prompt(&mut r, 600), "the game reaches its prompt");
    assert_eq!(leader_name(&r), "Zhuge");
    let s = r.e.state();
    assert_eq!(s.actors.attrs.level[1], 3);
    assert_ne!(s.actors.objlist_tail[0x81], 0, "portrait byte");
    let hits = r.e.stub_hits();
    assert!(
        hits.keys().all(|k| k.starts_with("u ")),
        "only u's declared seams: {hits:?}"
    );
}

#[test]
fn quit_in_the_menu_ends_the_launcher() {
    let Some(files) = files() else { return };
    let mut r = Run::launcher(files, 0);
    to_menu(&mut r);
    assert!(!r.e.finished());
    r.keys(&[Key::Char('\u{11}')], 10);
    assert!(
        r.e.finished(),
        "^Q: u exits with code 3, the launcher stops"
    );
    assert_eq!(program(&r), Program::U, "game never ran");
}

/// The character `u` writes goes the way of a Save: the platform's store gets
/// `savegame/objlist`, the next boot lays it over the files, and Journey Onward plays it.
#[test]
fn a_created_character_is_stored_and_journey_onward_after_a_reload_plays_it() {
    let Some(files) = files() else { return };
    let mut r = Run::launcher(files.clone(), 0);
    to_menu(&mut r);
    create_from_menu(&mut r, |_, _| {});
    assert!(to_prompt(&mut r, 600));
    assert_eq!(leader_name(&r), "Zhuge");
    {
        let s = r.e.state();
        assert_eq!(s.saveload.generation, 1, "u's write is a save generation");
        let names: Vec<String> = changed_since(&s, 0).into_keys().collect();
        assert_eq!(names, [OBJLIST_FILE], "only the objlist");
    }
    let mut store = MemStore::default();
    let mut sync = SaveSync::default();
    assert_eq!(sync.flush(&r.e, &mut store).unwrap(), 1);
    let objlist = &store.files[OBJLIST_FILE];
    assert_eq!(&objlist[0xf00..0xf06], b"Zhuge\0");
    assert_ne!(objlist[0x1c72], 0, "the portrait byte: a character exists");

    let mut again: Files = files;
    assert_eq!(attach(&mut again, &store).unwrap(), 1);
    let mut r = Run::launcher(again, 1);
    to_menu(&mut r);
    r.keys(&[Key::Char('j')], 30);
    assert_eq!(program(&r), Program::Game);
    assert!(to_prompt(&mut r, 600), "{:?}", said(&r));
    assert!(!said(&r).contains(NO_CHARACTER), "{:?}", said(&r));
    assert_eq!(leader_name(&r), "Zhuge", "the stored character");
}

#[test]
fn without_u_the_game_starts_at_once() {
    if files().is_none() {
        return;
    }
    let zip = std::fs::read(super::real::ZIP).unwrap();
    let mut map: BTreeMap<String, Vec<u8>> = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data: zip,
    }])
    .files;
    let n = map.len();
    map.retain(|path, _| {
        !path
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .eq_ignore_ascii_case("u")
    });
    assert_eq!(map.len() + 1, n, "one file named u");
    let mut r = Run::launcher(Files::new(map), 0);
    assert!(to_prompt(&mut r, 120), "game at once");
    assert_eq!(program(&r), Program::Game);
    assert_eq!(leader_name(&r), "Avatar", "the shipped disk state");
}

/// True once the published frame shows a colour other than black.
fn visible(r: &Run) -> bool {
    let (idx, pal) = r.frame();
    idx.iter().any(|&i| pal[usize::from(i)] != (0, 0, 0))
}

/// Receipt: the launcher's screens as PNG + raw indices under `U6_RECEIPT_DIR` (default
/// `u6-launcher-receipt/` in the system temp directory): the first visible opening frame, the
/// menu, a
/// creation question, the game at its first prompt after the creation, and on a fresh disk
/// Journey Onward's message and the opening of the `u` the launcher runs next. Run with
/// `--ignored --nocapture`.
#[test]
#[ignore = "writes receipt images outside the repository"]
fn receipt_launcher_screens() {
    let Some(files) = files() else { return };
    let dir = std::env::var_os("U6_RECEIPT_DIR").map_or_else(
        || std::env::temp_dir().join("u6-launcher-receipt"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&dir).unwrap();
    let shot = |name: &str, r: &Run| {
        let (idx, pal) = r.frame();
        png(&dir.join(format!("{name}.png")), &idx, &pal);
        std::fs::write(dir.join(format!("{name}.idx")), &idx).unwrap();
        println!(
            "shot {name} at tick {} ({:?})",
            r.e.ticks(),
            r.e.state().main.program
        );
    };
    let mut r = Run::launcher(files.clone(), 0);
    while !visible(&r) {
        r.tick(1);
    }
    shot("01-opening-first", &r);
    to_menu(&mut r);
    shot("02-menu", &r);
    create_from_menu(&mut r, |name, run| {
        if name == "question" {
            shot("03-question", run);
        }
    });
    assert!(to_prompt(&mut r, 600));
    shot("04-game-after-creation", &r);
    println!(
        "leader {:?}; party {:?}",
        leader_name(&r),
        &r.e.state().actors.party.names[..4]
    );

    let mut r = Run::launcher(files, 0);
    to_menu(&mut r);
    r.keys(&[Key::Char('j')], 200);
    shot("05-no-character", &r);
    println!("messages {:?}", r.e.state().ui.messages);
    r.keys(&[Key::Space], 30);
    while !visible(&r) {
        r.tick(1);
    }
    shot("06-no-character-then-u", &r);
    println!("stubs {:?}", r.e.stub_hits());
    println!("dir {}", dir.display());
}

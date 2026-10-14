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
use crate::sound::med::PLAYING_SONG;
use crate::sound::{CHIP_BUFFER, MODULE_BUFFER};
use u6_testkit::assert_same;

/// Image address of the message `game` prints when the save holds no character (main pushes
/// it, 0x135452).
const MSG_CREATE_CHARACTER: u32 = 0x0013_50a6;

/// That message read from the user's `game`, up to its last word (the scroll holds the lines
/// the window wrapped it into; the last one stays open while the cursor waits after it).
fn no_character(r: &Run) -> String {
    let msg = crate::main_loop::game_string(r.e.io(), MSG_CREATE_CHARACTER);
    let msg = String::from_utf8_lossy(&msg);
    let msg = msg.trim();
    let head = msg.rsplit_once(' ').map_or(msg, |(head, _)| head);
    assert!(
        head.len() > 8,
        "the message is in game ({} bytes)",
        head.len()
    );
    head.to_string()
}

/// The size of the message scroll, for failure messages (the text itself is the game's).
fn lines(r: &Run) -> String {
    format!("{} message lines", r.e.state().ui.messages.len())
}

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
    assert!(said(&r).contains(&no_character(&r)), "{}", lines(&r));
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
    assert_same!(
        leader_name(&r),
        "Zhuge",
        "the leader is the created character"
    );
    let s = r.e.state();
    assert_eq!(s.actors.attrs.level[1], 3);
    assert_ne!(s.actors.objlist_tail[0x81], 0, "portrait byte");
    let hits = r.e.stub_hits();
    assert!(
        hits.keys().all(|k| k.starts_with("u ")),
        "only u's declared seams: {hits:?}"
    );
}

/// Whether the host's driver plays the module buffer with the file `name` in it (the player
/// writes a few bytes of the module as it plays).
fn plays(r: &Run, name: &str) -> bool {
    let Some(sp) = &r.speaker else { return false };
    let file = r.e.io().files().get(name).unwrap();
    let n = file.len().min(MODULE_BUFFER);
    let at = CHIP_BUFFER as usize;
    let buf = &sp.driver.machine.chip[at..at + n];
    let differ = buf.iter().zip(file).filter(|(a, b)| a != b).count();
    let p = &sp.driver.player;
    p.state == PLAYING_SONG && p.module.is_some() && differ * 100 < n
}

/// The songs reach the hosts' audio driver as `game`'s do (the engine's sound log, replayed
/// after each tick): `u`'s opening and menu songs, the player stopped when `u` exits, the songs
/// of the `u` that `game` sends the launcher back to, and the new `game`'s after the relaunch.
#[test]
fn the_songs_of_u_and_game_reach_the_hosts_audio_across_a_relaunch() {
    let Some(files) = files() else { return };
    let mut r = Run::launcher(files, 0).with_speaker();
    let song = |r: &Run, n: usize| {
        let s = crate::intro::state_for(r.e.io().files()).unwrap();
        String::from_utf8_lossy(&s.exe.song_names[n]).into_owned()
    };
    r.tick(30);
    assert!(
        plays(&r, &song(&r, 2)),
        "the opening's song from its first picture"
    );
    let opening = r.peak();
    to_menu(&mut r);
    assert!(plays(&r, &song(&r, 1)), "the menu's song");
    r.peak();
    r.tick(60);
    let menu = r.peak();
    r.keys(&[Key::Char('j')], 200);
    assert_eq!(program(&r), Program::Game);
    let sp = r.speaker.as_ref().unwrap();
    assert!(sp.driver.player.module.is_none(), "u stopped the player");
    r.keys(&[Key::Space], 30);
    assert_eq!(program(&r), Program::U);
    r.tick(30);
    assert!(plays(&r, &song(&r, 2)), "the second u's opening song");
    to_menu(&mut r);
    create_from_menu(&mut r, |_, _| {});
    assert!(to_prompt(&mut r, 600));
    let n = r.e.state().sound.current;
    let name = r.e.state().sound.file_name(n).map(str::to_owned);
    assert!(n != 0 && name.is_some(), "the new game plays song {n}");
    assert!(
        plays(&r, &name.unwrap()),
        "the new game's song {n} reaches the driver"
    );
    println!("peaks: opening {opening:.3}, menu {menu:.3}");
    assert!(opening > 0.01 && menu > 0.01);
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
    assert_same!(
        leader_name(&r),
        "Zhuge",
        "the leader is the created character"
    );
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
    assert_same!(objlist[0xf00..0xf06], b"Zhuge\0", "the stored name");
    assert_ne!(objlist[0x1c72], 0, "the portrait byte: a character exists");

    let mut again: Files = files;
    assert_eq!(attach(&mut again, &store).unwrap(), 1);
    let mut r = Run::launcher(again, 1);
    to_menu(&mut r);
    r.keys(&[Key::Char('j')], 30);
    assert_eq!(program(&r), Program::Game);
    assert!(to_prompt(&mut r, 600), "{}", lines(&r));
    assert!(!said(&r).contains(&no_character(&r)), "{}", lines(&r));
    assert_same!(leader_name(&r), "Zhuge", "the stored character");
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
    let files = Files::new(map);
    let shipped = files.get(OBJLIST_FILE).unwrap()[0xf00..]
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect::<String>();
    let mut r = Run::launcher(files, 0);
    assert!(to_prompt(&mut r, 120), "game at once");
    assert_eq!(program(&r), Program::Game);
    assert!(!shipped.is_empty());
    assert!(
        leader_name(&r) == shipped,
        "the shipped disk state's leader"
    );
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
        "leader has the typed name: {}; party of {}",
        leader_name(&r) == "Zhuge",
        r.e.state().actors.party.size
    );

    let mut r = Run::launcher(files, 0);
    to_menu(&mut r);
    r.keys(&[Key::Char('j')], 200);
    shot("05-no-character", &r);
    println!("{}", lines(&r));
    r.keys(&[Key::Space], 30);
    while !visible(&r) {
        r.tick(1);
    }
    shot("06-no-character-then-u", &r);
    println!("stubs {:?}", r.e.stub_hits());
    println!("dir {}", dir.display());
}

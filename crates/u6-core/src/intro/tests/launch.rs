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
use crate::sound::{AudioDriver, CHIP_BUFFER, CHIP_EFFECT, MODULE_BUFFER};
use u6_testkit::Quiet;
use u6_testkit::assert_same;

/// Image address of the message `game` prints when the save holds no character (main pushes
/// it, 0x135452).
const MSG_CREATE_CHARACTER: u32 = 0x0013_50a6;

/// That message read from the user's `game`, up to its last word (the scroll holds the lines
/// the window wrapped it into; the last one stays open while the cursor waits after it).
fn no_character(r: &Run) -> String {
    let msg = crate::main_loop::game_string(
        r.e.io(),
        crate::string_registry::fixture((MSG_CREATE_CHARACTER) as i64, false),
    );
    let msg = String::from_utf8_lossy(&msg);
    let msg = msg.trim();
    let head = msg.rsplit_once(' ').map_or(msg, |(head, _)| head);
    assert!(head.len() > 8, "the message is in game");
    head.to_string()
}

/// The failure note for a check of the message scroll (its lines and their count are the
/// game's, so the note names the scroll only).
fn lines(_: &Run) -> String {
    "the message scroll".to_string()
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
    assert!(program(&r) == Program::U, "the launcher runs u first");
    to_menu(&mut r);
    r.keys(&[Key::Char('j')], 200);
    assert!(program(&r) == Program::Game, "Journey Onward starts game");
    assert!(said(&r).contains(&no_character(&r)), "{}", lines(&r));
    assert!(!r.e.finished());
    // A key: game exits with code 0, the launcher's state 0 runs u again from its opening.
    r.keys(&[Key::Space], 30);
    assert!(program(&r) == Program::U);
    assert!(!r.e.finished());
    // The second u makes a character; the second game (a new process) plays it.
    to_menu(&mut r);
    create_from_menu(&mut r, |_, _| {});
    assert!(program(&r) == Program::Game);
    assert!(to_prompt(&mut r, 600), "the game reaches its prompt");
    assert_same!(
        leader_name(&r),
        "Zhuge",
        "the leader is the created character"
    );
    let s = r.e.state();
    assert!(s.actors.attrs.level[1] == 3);
    assert!(s.actors.objlist_tail[0x81] != 0, "portrait byte");
    let hits = r.e.stub_hits();
    assert!(
        hits.keys().all(|k| k.starts_with("u ")),
        "only u's declared seams: {:?}",
        hits.keys().collect::<Vec<_>>()
    );
}

/// Whether the host's driver plays the module buffer with the file `name` in it (the player
/// writes a few bytes of the module as it plays).
fn plays(r: &Run, name: &str) -> bool {
    let Some(sp) = &r.speaker else { return false };
    let file = r.e.io().files().get(name).quiet();
    let n = file.len().min(MODULE_BUFFER);
    let at = CHIP_BUFFER as usize;
    let buf = &sp.driver.machine.chip[at..at + n];
    let differ = buf.iter().zip(file).filter(|(a, b)| a != b).count();
    let p = &sp.driver.player;
    p.state == PLAYING_SONG && p.module.is_some() && differ * 100 < n
}

/// How far the host's driver is from what a new `game` process's player starts from once the
/// process's song `name` is in (setup 0x14ef06: a module buffer of its own, the effect module
/// as the image holds it): the number of nonzero buffer bytes past that file (bytes of an
/// earlier program's files) and whether the effect module is the image's.
fn new_process_left(r: &Run, name: &str) -> (usize, bool) {
    let Some(sp) = &r.speaker else {
        return (usize::MAX, false);
    };
    let files = r.e.io().files();
    let n = files.get(name).quiet().len().min(MODULE_BUFFER);
    let (at, fx) = (CHIP_BUFFER as usize, CHIP_EFFECT as usize);
    let chip = &sp.driver.machine.chip;
    let image = AudioDriver::new(files).quiet();
    let left = chip[at + n..at + MODULE_BUFFER]
        .iter()
        .filter(|&&b| b != 0)
        .count();
    (left, chip[fx..] == image.machine.chip[fx..])
}

/// The song the game state plays and its file name.
fn game_song(r: &Run) -> (u16, String) {
    let n = r.e.state().sound.current;
    let name = r.e.state().sound.file_name(n).map(str::to_owned);
    assert!(n != 0 && name.is_some(), "the game plays song {n}");
    (n, name.quiet())
}

/// File name of `u`'s song `n` (from its song table).
fn u_song(r: &Run, n: usize) -> String {
    let s = crate::intro::state_for(r.e.io().files()).quiet();
    String::from_utf8_lossy(&s.exe.song_names[n]).into_owned()
}

/// Stands for what `u`'s process leaves in the memory it frees at its exit (cleanup 0x113b68):
/// a nonzero byte in the host's driver everywhere past `u`'s song `name` in the module buffer
/// and over the effect module, none of which a `game` process may start from.
fn fill_u_memory(r: &mut Run, name: &str) {
    let n = r.e.io().files().get(name).quiet().len().min(MODULE_BUFFER);
    let (at, fx) = (CHIP_BUFFER as usize, CHIP_EFFECT as usize);
    let chip = &mut r.speaker.as_mut().quiet().driver.machine.chip;
    chip[at + n..at + MODULE_BUFFER].fill(0xa5);
    chip[fx..].fill(0xa5);
}

/// The first `game` after `u` is a new process, as every later one is: `u`'s exit (cleanup
/// 0x113b68) frees its module buffer and `game`'s setup (0x14ef06) allocates its own. After
/// `u`'s opening and menu songs, whatever `u`'s memory holds, and its creation, the game's song
/// plays from a buffer that holds its own file only, beside the image's effect module: what the
/// relaunch gives ([`the_songs_of_u_and_game_reach_the_hosts_audio_across_a_relaunch`]).
#[test]
fn the_first_game_after_u_starts_the_player_over_as_a_relaunch_does() {
    let Some(files) = files() else { return };
    let mut r = Run::launcher(files, 0).with_speaker();
    to_menu(&mut r);
    let menu = u_song(&r, 1);
    assert!(plays(&r, &menu), "the menu's song");
    fill_u_memory(&mut r, &menu);
    create_from_menu(&mut r, |_, _| {});
    assert!(program(&r) == Program::Game);
    assert!(to_prompt(&mut r, 600));
    let (n, name) = game_song(&r);
    assert!(plays(&r, &name), "the game's song {n} reaches the driver");
    assert!(
        new_process_left(&r, &name) == (0, true),
        "(bytes of u's memory past the game's song {n}, the image's effect module)"
    );
}

/// The songs reach the hosts' audio driver as `game`'s do (the engine's sound log, replayed
/// after each tick): `u`'s opening and menu songs, the player stopped when `u` exits, the songs
/// of the `u` that `game` sends the launcher back to, and the new `game`'s after the relaunch.
#[test]
fn the_songs_of_u_and_game_reach_the_hosts_audio_across_a_relaunch() {
    let Some(files) = files() else { return };
    let mut r = Run::launcher(files, 0).with_speaker();
    r.tick(30);
    assert!(
        plays(&r, &u_song(&r, 2)),
        "the opening's song from its first picture"
    );
    let opening = r.peak();
    to_menu(&mut r);
    assert!(plays(&r, &u_song(&r, 1)), "the menu's song");
    r.peak();
    r.tick(60);
    let menu = r.peak();
    r.keys(&[Key::Char('j')], 200);
    assert!(program(&r) == Program::Game);
    let sp = r.speaker.as_ref().quiet();
    assert!(sp.driver.player.module.is_none(), "u stopped the player");
    r.keys(&[Key::Space], 30);
    assert!(program(&r) == Program::U);
    r.tick(30);
    assert!(plays(&r, &u_song(&r, 2)), "the second u's opening song");
    to_menu(&mut r);
    let menu_song = u_song(&r, 1);
    fill_u_memory(&mut r, &menu_song);
    create_from_menu(&mut r, |_, _| {});
    assert!(to_prompt(&mut r, 600));
    let (n, name) = game_song(&r);
    assert!(
        plays(&r, &name),
        "the new game's song {n} reaches the driver"
    );
    assert!(
        new_process_left(&r, &name) == (0, true),
        "no byte of u's memory past the new game's song {n}, and the image's effect module"
    );
    assert!(
        opening > 0.01 && menu > 0.01,
        "the opening and the menu are audible"
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
    assert!(program(&r) == Program::U, "game never ran");
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
        assert!(s.saveload.generation == 1, "u's write is a save generation");
        let names: Vec<String> = changed_since(&s, 0).into_keys().collect();
        assert!(names == [OBJLIST_FILE], "only the objlist");
    }
    let mut store = MemStore::default();
    let mut sync = SaveSync::default();
    assert!(sync.flush(&r.e, &mut store).quiet() == 1);
    let objlist = &store.files[OBJLIST_FILE];
    assert_same!(objlist[0xf00..0xf06], b"Zhuge\0", "the stored name");
    assert!(
        objlist[0x1c72] != 0,
        "the portrait byte: a character exists"
    );

    let mut again: Files = files;
    assert!(attach(&mut again, &store).quiet() == 1);
    let mut r = Run::launcher(again, 1);
    to_menu(&mut r);
    r.keys(&[Key::Char('j')], 30);
    assert!(program(&r) == Program::Game);
    assert!(to_prompt(&mut r, 600), "{}", lines(&r));
    assert!(!said(&r).contains(&no_character(&r)), "{}", lines(&r));
    assert_same!(leader_name(&r), "Zhuge", "the stored character");
}

#[test]
fn without_u_the_game_starts_at_once() {
    if files().is_none() {
        return;
    }
    let zip = std::fs::read(u6_testkit::archive()).quiet();
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
    assert!(map.len() + 1 == n, "one file named u");
    let files = Files::new(map);
    let shipped = files.get(OBJLIST_FILE).quiet()[0xf00..]
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect::<String>();
    let mut r = Run::launcher(files, 0);
    assert!(to_prompt(&mut r, 120), "game at once");
    assert!(program(&r) == Program::Game);
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
    std::fs::create_dir_all(&dir).quiet();
    let shot = |name: &str, r: &Run| {
        let (idx, pal) = r.frame();
        png(&dir.join(format!("{name}.png")), &idx, &pal);
        std::fs::write(dir.join(format!("{name}.idx")), &idx).quiet();
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
    println!("leader has the typed name: {}", leader_name(&r) == "Zhuge");

    let mut r = Run::launcher(files, 0);
    to_menu(&mut r);
    r.keys(&[Key::Char('j')], 200);
    shot("05-no-character", &r);
    r.keys(&[Key::Space], 30);
    while !visible(&r) {
        r.tick(1);
    }
    shot("06-no-character-then-u", &r);
    println!("stubs {:?}", r.e.stub_hits().keys().collect::<Vec<_>>());
    println!("dir {}", dir.display());
}

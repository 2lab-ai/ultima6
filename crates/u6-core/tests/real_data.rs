//! Real-data boot: the user's archive.org zip → u6-intake → `Files` → `State::new_game` →
//! `Engine`. Runs only where the zip exists (never in CI: the data is not ours to ship; it is
//! read here only, never copied, and the test prints numbers only).
#![allow(clippy::disallowed_methods)] // tests may read files

use std::path::Path;

use u6_core::{Engine, Files, InputEvent, Key, State};
use u6_formats::objlist::ObjList;
use u6_intake::{InputFile, intake};

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

#[test]
fn new_game_from_the_real_zip() {
    let Some(files) = files() else { return };
    let state = State::new_game(&files).unwrap_or_else(|e| panic!("{e}"));
    let raw = files.get("savegame/objlist").unwrap();
    let objlist = ObjList::parse(raw).unwrap();

    // The party and the clock against the raw objlist (formats.md §6): the size at 0x0FF0, the
    // roster at 0x0FE0, the BE year at 0x1BF7.
    let party = &state.actors.party;
    let size = raw[0x0FF0];
    assert!(party.size == size, "the party size at 0x0FF0");
    assert_eq!(state.actors.party.active().count(), usize::from(size));
    let first = u16::from(raw[0x0FE0]);
    assert!(
        party.leader().map(|id| id.0) == Some(first),
        "the leader is the roster's first"
    );
    let leader = state.leader_position().unwrap();
    assert!(
        leader == objlist.actors[usize::from(first)].pos,
        "the leader stands where the objlist puts it"
    );
    assert!(state.map.centre == leader, "the view on the leader");
    let year = u16::from_be_bytes([raw[0x1BF7], raw[0x1BF8]]);
    assert!(state.actors.calendar.year == year, "the year at 0x1BF7");

    // The 69 block files and the 256 actors' schedule starts (the code's counts).
    assert_eq!(state.objects.blocks.len(), u6_core::objects::BLOCK_COUNT);
    assert_eq!(state.actors.schedules.first.len(), 256);
    assert_eq!(
        usize::from(state.actors.schedules.total),
        state.actors.schedules.entries.len()
    );
    assert_eq!(
        state.map.anim_enabled.len(),
        usize::from(state.tables.anim.count)
    );
    // The leader stands on a map tile.
    let (x, y, z) = (leader.x.into(), leader.y.into(), leader.z.into());
    assert!(state.map.tile_at(x, y, z).is_some());
    println!(
        "leader actor 1 at ({}, {}, {}); calendar {:?}; {} schedule entries; {} of {} anim slots on",
        leader.x,
        leader.y,
        leader.z,
        state.actors.calendar,
        state.actors.schedules.entries.len(),
        state.map.anim_enabled.iter().filter(|&&on| on != 0).count(),
        state.map.anim_enabled.len(),
    );
}

#[test]
fn engine_boots_draws_and_moves_the_view() {
    let Some(files) = files() else { return };
    let mut engine = Engine::game_first(files, 0).unwrap_or_else(|e| panic!("{e}"));
    engine.tick(&[], 16);
    let start = engine.state().map.centre;
    let first = engine.frame().pixels().to_vec();
    assert!(first.iter().any(|&p| p != 0), "map drawn");
    engine.tick(&[InputEvent::Key(Key::Right)], 16);
    engine.tick(&[], 16);
    assert_eq!(engine.state().map.centre.x, start.x + 1);
    assert!(engine.frame().pixels() != &first[..], "view moved");
    // Stub contract, independent of which stubs still exist: one log line per stub id hit.
    let hits = engine.stub_hits();
    let log = engine.take_log();
    let logged: Vec<&str> = log.iter().map(|l| l.trim_start_matches("stub: ")).collect();
    let mut ids: Vec<&str> = hits.keys().copied().collect();
    let mut sorted = logged.clone();
    sorted.sort_unstable();
    ids.sort_unstable();
    assert_eq!(sorted, ids, "{log:?}");
    assert!(engine.take_log().is_empty());
}

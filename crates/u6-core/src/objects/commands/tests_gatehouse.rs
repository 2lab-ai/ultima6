//! Synthetic-data tests of the gatehouse plants (port-only, owner 2026-10-21): a made-up map,
//! objects and strings, nothing from the game.

use super::gatehouse::PLANT_TILES;
use super::*;
use crate::engine::{InputEvent, Key};
use crate::objects::tests::{put, world};
use crate::objects::{HEAD, IN_INVENTORY, OBJECT_SLOTS, Slot, TEMPORARY, move_into, remove};
use crate::state::State;
use u6_formats::look::LookEntry;

/// The made-up lock of the door and the crank's (and bridge's) quality.
const LOCK: u8 = 0x21;
const BRIDGE_Q: u8 = 0x07;
/// The door, the crank south-west of it by (−1, +2), a plant three tiles north of the door, one
/// on the crank's side and one a tile past [`super::gatehouse::PLANT_REACH`].
const DOOR: (u16, u16) = (500, 500);
const CRANK: (u16, u16) = (499, 502);
const PLANT: (u16, u16) = (500, 497);
const CRANK_SIDE_PLANT: (u16, u16) = (497, 501);
const FAR_PLANT: (u16, u16) = (507, 497);
/// Beside [`PLANT`] (and beside [`OTHER_TILE`]).
const BESIDE: (u16, u16) = (500, 498);
const OTHER_TILE: (u16, u16) = (501, 497);

/// The strings Look and the search print, each its own marker (no escape characters; the full
/// stop has four bytes before the lead-in).
fn strings() -> Vec<(u32, &'static [u8])> {
    vec![
        (0x0012_8444, b"see \0"),
        (0x0012_8440, b".\n\0"),
        (0x0012_8402, b"searching \0"),
        (0x0012_8468, b"nothing found\n\0"),
    ]
}

/// A tile that is not a plant.
fn ground() -> u8 {
    (0..=u8::MAX).find(|t| !PLANT_TILES.contains(t)).unwrap()
}

/// The gatehouse world: the leader at `leader` with the view on it, the locked door (frame 9)
/// with its lock, the plants on the map, and the crank with a bridge piece of its quality
/// (`bridge` = its quality) when `crank`.
fn gatehouse(leader: (u16, u16), crank: bool, bridge: u8) -> State {
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    s.objcmds = ObjCommands::from_data(GameData::with_bytes(&strings()));
    s.magic.tables.doors = [0x129, 0x12a, 0x12b, 0x12c];
    s.tables.look.entries = vec![
        LookEntry {
            tile: 0xff,
            text: "spot".into(),
        },
        LookEntry {
            tile: 0x40 * 4,
            text: "kname".into(),
        },
    ];
    s.map.centre.x = leader.0;
    s.map.centre.y = leader.1;
    s.actors.party.size = 1;
    s.actors.party.members[0] = 1;
    s.objects.type_frame[1] = 0x19a;
    s.objects.set_xyz(1, leader.0, leader.1, 0);
    crate::main_loop::test_support::link_placed_objects(&mut s);
    let d = put(&mut s, 0x12c, DOOR.0, DOOR.1);
    s.objects.type_frame[d as usize] = 0x12c | 9 << 10;
    s.objects.qq[d as usize] = u16::from(LOCK) << 8;
    if crank {
        let c = put(&mut s, 0x120, CRANK.0, CRANK.1);
        s.objects.qq[c as usize] = u16::from(BRIDGE_Q) << 8;
        let b = put(&mut s, 0x10d, 497, 503);
        s.objects.qq[b as usize] = u16::from(bridge) << 8;
    }
    s.map.window_tiles = vec![ground(); crate::map::WINDOW_CELLS];
    for (x, y) in [PLANT, CRANK_SIDE_PLANT, FAR_PLANT] {
        let [ox, oy] = s.map.window_origin;
        let at = crate::map::cell(usize::from(x - ox), usize::from(y - oy));
        s.map.window_tiles[at] = PLANT_TILES[0];
    }
    s
}

/// Everything the message scroll took: its lines and the one being written.
fn scroll_text(s: &State) -> String {
    let t = &s.ui.text;
    let mut all = s.ui.messages.join("\n");
    all.extend(
        t.transcript_line
            .iter()
            .map(|&b| crate::ui::wide::latin_char(b)),
    );
    all.extend(
        t.line[usize::from(t.line_start)..usize::from(t.line_len)]
            .iter()
            .map(|&b| crate::ui::wide::latin_char(b)),
    );
    all
}

/// Look at the map tile (x, y) (no object selected) on a fresh scroll: the state after it and
/// what it printed.
fn look_at(mut s: State, (x, y): (u16, u16)) -> (State, String) {
    let input = s.ui.input.clone();
    s.ui = crate::main_loop::test_support::test_ui();
    s.ui.input = input;
    let c = s.map.centre;
    let (hw, hh) = (crate::geom::HALF_W as i16, crate::geom::HALF_H as i16);
    let (col, row) = (x as i16 - c.x as i16 + hw, y as i16 - c.y as i16 + hh);
    // The middle of its view cell on the frame, the map drawn from the wide map's pixel origin
    // (MAP_X, MAP_Y; spec 13 Geometry).
    let (mx, my) = (crate::geom::MAP_X as i16, crate::geom::MAP_Y as i16);
    s.ui.input.selection = -1;
    s.ui.input.pointer = (mx + col * 16 + 8, my + row * 16 + 8);
    s.ui.input.target_void = false;
    let mut e = crate::Engine::with_game(s, crate::Files::default(), |st, io| async move {
        look(&st, &io).await;
    });
    for _ in 0..64 {
        if e.finished() {
            break;
        }
        let key = if e.io().pending_events() == 0 {
            vec![InputEvent::Key(Key::Enter)]
        } else {
            Vec::new()
        };
        e.tick(&key, 16);
    }
    assert!(e.finished(), "Look did not end");
    let s = e.state().clone();
    let text = scroll_text(&s);
    (s, text)
}

/// The keys (type 0x40) of quality `q` the port holds, anywhere.
fn keys(s: &State, q: u8) -> Vec<Slot> {
    let o = &s.objects;
    let mut v = Vec::new();
    let mut c = o.next_of(HEAD);
    let mut n = 0;
    while c >= 0 && n < OBJECT_SLOTS {
        if o.kind(c) == 0x40 && o.qq_of(c) >> 8 == u16::from(q) {
            v.push(c);
        }
        c = o.next_of(c);
        n += 1;
    }
    v
}

/// The search's find, in order: the tile's name and full stop, the lead-in, the key's name,
/// the full stop; never the found-nothing line.
fn gave_key(text: &str) -> bool {
    text.contains("spot.\nsearching kname.") && !text.contains("nothing found")
}

#[test]
fn a_gatehouse_plant_beside_the_leader_gives_one_key_of_the_doors_lock() {
    let s = gatehouse(BESIDE, true, BRIDGE_Q);
    assert!(keys(&s, LOCK).is_empty());
    let (s, text) = look_at(s, PLANT);
    assert!(gave_key(&text), "{text:?}");
    let made = keys(&s, LOCK);
    assert_eq!(made.len(), 1, "one key");
    let key = made[0];
    let o = &s.objects;
    assert_eq!((o.place(key), o.x(key), o.y(key)), (0, PLANT.0, PLANT.1));
    assert_eq!(
        o.qq_of(key),
        u16::from(LOCK) << 8,
        "the quality, as add-object"
    );
    assert_eq!(o.status(key) & 0x01, 0x01, "fine to take");
    assert_eq!(o.status(key) & TEMPORARY, 0, "an ordinary map object");

    // While it lies there: the found-nothing line, no second key.
    let (mut s, text) = look_at(s, PLANT);
    assert!(text.contains("spot.\nnothing found"), "{text:?}");
    assert!(!text.contains("searching"), "{text:?}");
    assert_eq!(keys(&s, LOCK), [key]);

    // Carried by the leader: nothing either.
    move_into(&mut s, key, 1, IN_INVENTORY);
    let (mut s, text) = look_at(s, PLANT);
    assert!(text.contains("nothing found"), "{text:?}");
    assert_eq!(keys(&s, LOCK), [key]);

    // Gone: the plant gives a new one.
    remove(&mut s, key);
    assert!(keys(&s, LOCK).is_empty());
    let (s, text) = look_at(s, PLANT);
    assert!(gave_key(&text), "{text:?}");
    let again = keys(&s, LOCK);
    assert_eq!(again.len(), 1);
    assert_eq!(s.objects.place(again[0]), 0);
}

#[test]
fn a_plant_on_the_cranks_side_of_the_door_gives_the_key_too() {
    let (s, text) = look_at(gatehouse((498, 501), true, BRIDGE_Q), CRANK_SIDE_PLANT);
    assert!(gave_key(&text), "{text:?}");
    let made = keys(&s, LOCK);
    assert_eq!(made.len(), 1, "one key");
    let o = &s.objects;
    assert_eq!(
        (o.place(made[0]), o.x(made[0]), o.y(made[0])),
        (0, CRANK_SIDE_PLANT.0, CRANK_SIDE_PLANT.1)
    );
}

#[test]
fn the_gatehouse_key_comes_from_no_other_look() {
    // Not beside the leader: the tile's name and full stop only, as the original.
    let (s, text) = look_at(gatehouse((502, 499), true, BRIDGE_Q), PLANT);
    assert!(text.contains("spot."), "{text:?}");
    assert!(!text.contains("searching") && !text.contains("nothing found"));
    assert!(keys(&s, LOCK).is_empty(), "not beside the leader");
    // Every other case beside the leader: the found-nothing line and no key.
    for (what, s, at) in [
        (
            "a plant too far from the door",
            gatehouse((506, 498), true, BRIDGE_Q),
            FAR_PLANT,
        ),
        (
            "a tile that is no plant",
            gatehouse(BESIDE, true, BRIDGE_Q),
            OTHER_TILE,
        ),
        (
            "a door with no crank",
            gatehouse(BESIDE, false, BRIDGE_Q),
            PLANT,
        ),
        (
            "a crank with no bridge piece of its quality",
            gatehouse(BESIDE, true, BRIDGE_Q + 1),
            PLANT,
        ),
    ] {
        let (s, text) = look_at(s, at);
        assert!(text.contains("spot.\nnothing found"), "{what}: {text:?}");
        assert!(!text.contains("searching"), "{what}: {text:?}");
        assert!(keys(&s, LOCK).is_empty(), "{what}: no key");
    }
}

#[test]
fn a_door_of_no_lock_or_not_locked_has_no_gatehouse_plants() {
    for (frame, lock) in [(9u16, 0u8), (5, LOCK), (1, LOCK), (13, LOCK)] {
        let mut s = gatehouse(BESIDE, true, BRIDGE_Q);
        let d = crate::objects::object_of_kind_at(&mut s, DOOR.0, DOOR.1, 0, 0x12c);
        s.objects.type_frame[d as usize] = 0x12c | frame << 10;
        s.objects.qq[d as usize] = u16::from(lock) << 8;
        let (s, text) = look_at(s, PLANT);
        assert!(text.contains("nothing found"), "frame {frame}: {text:?}");
        assert!(
            keys(&s, lock).is_empty() && keys(&s, LOCK).is_empty(),
            "frame {frame}, lock {lock}: no key"
        );
    }
}

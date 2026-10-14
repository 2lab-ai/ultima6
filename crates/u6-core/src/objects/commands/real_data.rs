//! Real data: the user's archive.org zip → u6-intake → `Files` → `State::new_game`, then the
//! object commands. Skips when the zip is absent; reads only, prints numbers and names only.
#![allow(clippy::disallowed_methods)] // the test reads the zip

use std::path::Path;

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

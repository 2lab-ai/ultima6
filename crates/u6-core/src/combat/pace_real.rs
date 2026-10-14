//! Real data: the pace of the start fight against the original's. The game booted as the
//! oracle's game-first disks start it (the shipped start state, its combat byte set), then party
//! steps by arrow keys; per turn the engine time from the key to the next prompt, and what the
//! waits were for. Skips without the zip; prints times and counts only.
#![allow(clippy::disallowed_methods)] // the test reads the zip

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use u6_intake::{InputFile, intake};

use super::present::{
    FLASH_MS, FLIGHT_START_MS, FLIGHT_STEP_MS, FRAME_MS, NAME_MS, PRINT_MS, REFRESH_MS,
};
use crate::actors::turn::{AFTER_TURN_MS, COMBAT_NPC_MS, PICK_MS};
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

/// The steps of the oracle's runs: north out of the start position, then east and west.
const KEYS: [Key; 7] = [
    Key::Up,
    Key::Right,
    Key::Left,
    Key::Right,
    Key::Left,
    Key::Right,
    Key::Left,
];

/// The original in the same fight (tools/oracle/fight.mjs: game-first disks, 3 boots, the steps
/// of [`KEYS`]): per step, the PAL frames from the key taken (A4-0x7a2c cleared) to the next
/// prompt, and how many of them the game waited on the floppy (other tasks running: the first
/// step loads a map region and the object names).
const ORACLE: [[(u32, u32); 7]; 3] = [
    [
        (535, 137),
        (396, 0),
        (300, 0),
        (283, 0),
        (364, 0),
        (270, 0),
        (215, 0),
    ],
    [
        (439, 135),
        (334, 0),
        (169, 0),
        (137, 0),
        (261, 0),
        (138, 0),
        (255, 0),
    ],
    [
        (440, 140),
        (336, 0),
        (169, 0),
        (133, 0),
        (277, 0),
        (180, 0),
        (317, 0),
    ],
];

/// Seeds whose runs differ (the seed word picks the RNG state; 2 and 3 give the same run).
const SEEDS: [u16; 9] = [0, 2, 5, 7, 11, 100, 1000, 4242, 31337];

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        println!("skipped: no game files ({ZIP} not present)");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// One turn as the oracle measures it: from the tick the key goes in to the first tick where
/// the command loop reads again with nothing queued.
struct Turn {
    ms: u64,
    moved: bool,
    /// The waits of the turn by length (ms → how many, [`crate::combat::CombatState::paced`]).
    paced: BTreeMap<u16, u32>,
}

fn at_prompt(e: &Engine) -> bool {
    e.io().pending_events() == 0 && e.state().main.reading == 1
}

fn leader(e: &Engine) -> (u16, u16) {
    let p = e.state().leader_position().unwrap_or_default();
    (p.x, p.y)
}

/// Boots `game` at once with `seed`, settles at the prompt, then one turn per key of `keys` at
/// host ticks of `tick_ms`; with `zero_polls`, a poll with no time elapsed after each tick of a
/// turn. (Not at the prompt: the key wait draws the cursor and animates once per display frame,
/// 0x105c8e, so more frames there are more of the game, as more time is in the original.) With
/// `town`, the oracle's town setup first (tools/oracle/game.mjs townSetup): the actors in the
/// assault worktype (8) passed over (status bit 0x10) and combat broken off with `b`.
fn play_keys(
    files: &Files,
    seed: u16,
    keys: &[Key],
    town: bool,
    tick_ms: u32,
    zero_polls: bool,
) -> Vec<Turn> {
    let mut state = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
    state.rng.seed(seed);
    let keep = Rc::new(RefCell::new(None));
    let k = Rc::clone(&keep);
    let mut e = Engine::with_game(state, files.clone(), move |st, io| {
        *k.borrow_mut() = Some(Rc::clone(&st));
        crate::main_loop::game_first(st, io)
    });
    let tick = |e: &mut Engine, input: &[InputEvent]| {
        e.tick(input, tick_ms);
        if zero_polls && !at_prompt(e) {
            e.tick(&[], 0);
        }
    };
    let settle = |e: &mut Engine| {
        for _ in 0..20_000 {
            if at_prompt(e) && e.ticks() > 10 {
                break;
            }
            tick(e, &[]);
        }
        assert!(at_prompt(e), "seed {seed}: the game reaches the prompt");
    };
    settle(&mut e);
    if town {
        let st = keep.borrow().clone().expect("the game holds the state");
        {
            let mut s = st.borrow_mut();
            for a in 0..0x100 {
                if s.actors.worktype[a] == 8 {
                    s.actors.attrs.status[a] |= 0x10;
                }
            }
        }
        tick(&mut e, &[InputEvent::Key(Key::Char('b'))]);
        for _ in 0..30 {
            tick(&mut e, &[]);
        }
        settle(&mut e);
    }
    let mut turns = Vec::new();
    for &k in keys {
        let from = leader(&e);
        let t0 = e.io().now_ms();
        let before = e.state().combat.paced.clone();
        tick(&mut e, &[InputEvent::Key(k)]);
        for _ in 0..100_000 {
            if at_prompt(&e) {
                break;
            }
            tick(&mut e, &[]);
        }
        assert!(at_prompt(&e), "seed {seed}: the turn ends at the prompt");
        let mut paced = e.state().combat.paced.clone();
        for (ms, n) in &before {
            *paced.entry(*ms).or_default() -= n;
        }
        paced.retain(|_, n| *n > 0);
        turns.push(Turn {
            ms: e.io().now_ms() - t0,
            moved: leader(&e) != from,
            paced,
        });
    }
    turns
}

/// [`play_keys`] with the fight's steps ([`KEYS`]).
fn play(files: &Files, seed: u16, tick_ms: u32, zero_polls: bool) -> Vec<Turn> {
    play_keys(files, seed, &KEYS, false, tick_ms, zero_polls)
}

fn median(v: &mut [u32]) -> u32 {
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    }
}

/// What a wait of `ms` is for (the constants have distinct values in this fight).
fn what(ms: u16) -> &'static str {
    match ms {
        AFTER_TURN_MS => "refresh after a turn",
        PICK_MS => "scheduler pick",
        v if v == PICK_MS + COMBAT_NPC_MS => "pick + combat NPC turn",
        NAME_MS => "object name",
        FLASH_MS => "hit flash",
        FLIGHT_START_MS => "flight start",
        FLIGHT_STEP_MS => "flight step",
        REFRESH_MS => "refresh (flight end)",
        PRINT_MS => "message",
        _ => "other",
    }
}

#[test]
fn the_start_fight_turns_take_the_originals_time() {
    let Some(files) = files() else {
        return;
    };
    let frames = |ms: u64| (ms / u64::from(FRAME_MS)) as u32;
    // The port: every seed, the steps that moved the party (a blocked step runs no world turn,
    // in the original either).
    let (mut all, mut first5) = (Vec::new(), Vec::new());
    let mut spent: BTreeMap<&str, (u32, u64)> = BTreeMap::new();
    for seed in SEEDS {
        let turns = play(&files, seed, 16, false);
        let row: Vec<String> = turns
            .iter()
            .map(|t| format!("{}{}", frames(t.ms), if t.moved { "" } else { "*" }))
            .collect();
        println!("port seed {seed}: PAL frames per step {}", row.join(" "));
        for (i, t) in turns.iter().enumerate().filter(|(_, t)| t.moved) {
            all.push(frames(t.ms));
            if i < 5 {
                first5.push(frames(t.ms));
            }
            for (&ms, &n) in &t.paced {
                let e = spent.entry(what(ms)).or_default();
                e.0 += n;
                e.1 += u64::from(ms) * u64::from(n);
            }
        }
    }
    let n = all.len() as f64;
    println!("port, per moving step ({n}): what the waits were for");
    for (k, (count, ms)) in &spent {
        println!(
            "  {k}: {:.1} waits, {:.1} PAL frames",
            f64::from(*count) / n,
            *ms as f64 / n / f64::from(FRAME_MS)
        );
    }
    let oracle = |steps: usize, disk: bool| -> Vec<u32> {
        ORACLE
            .iter()
            .flat_map(|run| run[..steps].iter())
            .map(|&(f, d)| if disk { f } else { f - d })
            .collect()
    };
    for (name, steps, port) in [
        ("first 5 steps", 5, &mut first5),
        ("all 7 steps", 7, &mut all),
    ] {
        let p = median(port);
        let o = median(&mut oracle(steps, true));
        let o_cpu = median(&mut oracle(steps, false));
        let off = f64::from(p) / f64::from(o) - 1.0;
        println!(
            "{name}: median PAL frames per step, original {o} ({o_cpu} without the floppy \
             waits), port {p} ({:+.1}%, {} steps)",
            off * 100.0,
            port.len()
        );
        assert!(off.abs() <= 0.15, "{name}: port {p} vs original {o}");
    }
}

#[test]
fn the_start_fight_takes_the_same_time_at_any_host_tick() {
    // Spec 03 §1: the same steps take the same engine time at 16 ms and 8 ms ticks, and polls
    // with no time elapsed add nothing (to within a host tick per step).
    let Some(files) = files() else {
        return;
    };
    for seed in [0, 5] {
        let base: Vec<u64> = play(&files, seed, 16, false).iter().map(|t| t.ms).collect();
        for (tick, zero) in [(8, false), (16, true)] {
            let other: Vec<u64> = play(&files, seed, tick, zero)
                .iter()
                .map(|t| t.ms)
                .collect();
            println!(
                "seed {seed}: ms per step at 16 ms ticks {base:?}, at {tick} ms (zero polls \
                 {zero}) {other:?}"
            );
            for (a, b) in base.iter().zip(&other) {
                assert!(a.abs_diff(*b) <= 16, "seed {seed}: {base:?} vs {other:?}");
            }
        }
    }
}

/// The original in the castle without the fight (tools/oracle/fight.mjs with U6_FIGHT_SETUP=town:
/// the start's assault actors passed over, combat broken off, the steps of [`TOWN_KEYS`]): PAL
/// frames from the key taken to the next prompt.
const ORACLE_TOWN: [u32; 7] = [28, 43, 33, 48, 32, 41, 32];

/// The steps of the oracle's town runs: south along the carpet and back.
const TOWN_KEYS: [Key; 7] = [
    Key::Down,
    Key::Down,
    Key::Down,
    Key::Down,
    Key::Down,
    Key::Up,
    Key::Up,
];

#[test]
fn the_castle_without_the_fight_takes_the_originals_time() {
    let Some(files) = files() else {
        return;
    };
    let frames = |ms: u64| (ms / u64::from(FRAME_MS)) as u32;
    let mut spent: BTreeMap<&str, (u32, u64)> = BTreeMap::new();
    let mut all = Vec::new();
    for seed in SEEDS {
        let turns = play_keys(&files, seed, &TOWN_KEYS, true, 16, false);
        let row: Vec<String> = turns
            .iter()
            .map(|t| format!("{}{}", frames(t.ms), if t.moved { "" } else { "*" }))
            .collect();
        println!("town seed {seed}: PAL frames per step {}", row.join(" "));
        for t in turns.iter().filter(|t| t.moved) {
            all.push(frames(t.ms));
            for (&ms, &n) in &t.paced {
                let e = spent.entry(what(ms)).or_default();
                e.0 += n;
                e.1 += u64::from(ms) * u64::from(n);
            }
        }
    }
    let n = all.len() as f64;
    println!("town, per moving step ({n}): what the waits were for");
    for (k, (count, ms)) in &spent {
        println!(
            "  {k}: {:.1} waits, {:.1} PAL frames",
            f64::from(*count) / n,
            *ms as f64 / n / f64::from(FRAME_MS)
        );
    }
    let p = median(&mut all);
    let o = median(&mut ORACLE_TOWN.to_vec());
    let off = f64::from(p) / f64::from(o) - 1.0;
    println!(
        "town: median PAL frames per step, original {o}, port {p} ({:+.1}%, {n} steps)",
        off * 100.0
    );
    assert!(off.abs() <= 0.15, "town: port {p} vs original {o}");
}

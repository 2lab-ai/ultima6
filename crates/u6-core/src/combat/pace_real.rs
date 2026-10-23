//! Real data: the pace of the start fight against the original's. The game booted as the
//! oracle's game-first disks start it (the shipped start state, its combat byte set), then party
//! steps by arrow keys; per turn the engine time from the key to the next prompt, and what the
//! waits were for. Skips without the zip. Prints and fails with timings, seeds, step indices and
//! counts the test made only (spec 01 §Rights): nothing read from the user's files.
#![allow(clippy::disallowed_methods)] // the test reads the zip

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use u6_intake::{InputFile, intake};
use u6_testkit::Quiet;

#[cfg(not(feature = "wide"))]
use super::present::FRAME_MS;
use super::present::{
    ACTOR_FLASH_MS, DISSOLVE_MS, FLASH_MS, FLIGHT_START_MS, FLIGHT_STEP_MS, NAME_MS, PRINT_MS,
    REFRESH_MS, WINDOW_MS,
};
use crate::actors::turn::{AFTER_TURN_MS, COMBAT_NPC_MS, MINUTE_MS, PICK_MS, SETUP_MS};
use crate::actors::walk::{FOLLOW_MS, PASS_MOVED_MS, PASS_STILL_MS, STEP_MS};
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::pacer::STEP_MS as HOST_STEP_MS;
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
#[cfg(not(feature = "wide"))]
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
#[cfg(not(feature = "wide"))]
const SEEDS: [u16; 9] = [0, 2, 5, 7, 11, 100, 1000, 4242, 31337];

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        println!("skipped: no game files ({ZIP} not present)");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
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
    // Read by the comparisons with the original's run only (Original builds).
    #[cfg_attr(feature = "wide", allow(dead_code))]
    moved: bool,
    /// The waits of the turn by length (ms → how many, [`crate::combat::CombatState::paced`]).
    #[cfg_attr(feature = "wide", allow(dead_code))]
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
    let mut state = State::new_game(files).quiet();
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
        // 480 ms for the break-off.
        for _ in 0..480 / tick_ms {
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

#[cfg(not(feature = "wide"))]
fn median(v: &mut [u32]) -> u32 {
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    }
}

/// What a wait of `ms` is for: a named constant, or a world-turn wait with passes. The world
/// turn pays its passes ([`PICK_MS`] each, shorter than a frame) with its next wait
/// (actors/turn.rs `world_turn`): a combat NPC turn's, a clock minute's, a refresh's, or its
/// setup's at the end; such a wait is named by that one, with passes (a run of passes alone is
/// never a wait). A wait that is more than one of these (a combat NPC turn with 12 passes is as
/// long as a window render) is "ambiguous", not one of them picked.
fn what(ms: u16) -> &'static str {
    let passes =
        |base: u16, least: u16| ms >= base + least * PICK_MS && (ms - base).is_multiple_of(PICK_MS);
    let named = [
        (STEP_MS + FOLLOW_MS, "party step and its followers"),
        (STEP_MS, "party step"),
        (NAME_MS, "object name"),
        (FLASH_MS, "hit flash"),
        (FLIGHT_START_MS, "flight start"),
        (FLIGHT_STEP_MS, "flight step"),
        (REFRESH_MS, "refresh (flight end)"),
        (PRINT_MS, "message"),
        (WINDOW_MS, "window render"),
        (DISSOLVE_MS, "dissolving window"),
        (ACTOR_FLASH_MS, "actor flash"),
        (PASS_MOVED_MS, "Pass's followers, moved"),
        (PASS_STILL_MS, "Pass's followers, none moved"),
    ];
    let sums = [
        (
            passes(AFTER_TURN_MS, 0),
            "refresh after a turn, with passes",
        ),
        (passes(MINUTE_MS, 1), "clock minute, with passes"),
        (passes(COMBAT_NPC_MS, 1), "combat NPC turn, with passes"),
        (passes(SETUP_MS, 1), "turn setup, with passes"),
    ];
    let mut found = named
        .iter()
        .filter(|&&(v, _)| v == ms)
        .map(|&(_, name)| name)
        .chain(sums.iter().filter(|&&(hit, _)| hit).map(|&(_, name)| name));
    match (found.next(), found.next()) {
        (Some(name), None) => name,
        (Some(_), Some(_)) => "ambiguous",
        (None, _) => "other",
    }
}

#[test]
fn a_wait_is_named_by_the_one_constant_or_sum_it_is_or_called_ambiguous() {
    // Each named constant names itself where nothing else adds up to it; the sums name their
    // main part; a wait that is two of them (window render = combat NPC turn + 12 passes) is
    // ambiguous.
    for (ms, name) in [
        (FLASH_MS, "hit flash"),
        (FLIGHT_START_MS, "flight start"),
        (FLIGHT_STEP_MS, "flight step"),
        (REFRESH_MS, "refresh (flight end)"),
        (PRINT_MS, "message"),
        (NAME_MS, "object name"),
        (DISSOLVE_MS, "dissolving window"),
        (ACTOR_FLASH_MS, "actor flash"),
        (PASS_MOVED_MS, "Pass's followers, moved"),
        (PASS_STILL_MS, "Pass's followers, none moved"),
        (STEP_MS, "party step"),
        (STEP_MS + FOLLOW_MS, "party step and its followers"),
        (
            AFTER_TURN_MS + 2 * PICK_MS,
            "refresh after a turn, with passes",
        ),
        (MINUTE_MS + PICK_MS, "clock minute, with passes"),
        (COMBAT_NPC_MS + PICK_MS, "combat NPC turn, with passes"),
        (SETUP_MS + PICK_MS, "turn setup, with passes"),
    ] {
        assert_eq!(what(ms), name, "{ms} ms");
    }
    assert_eq!(
        COMBAT_NPC_MS + 12 * PICK_MS,
        WINDOW_MS,
        "the sum that collides"
    );
    assert_eq!(what(WINDOW_MS), "ambiguous");
    assert_eq!(what(1), "other");
}

// A comparison with the original's run: Original only (spec 13 Tests).
#[cfg(not(feature = "wide"))]
#[test]
fn the_start_fight_turns_take_the_originals_time() {
    let Some(files) = files() else {
        return;
    };
    let frames = |ms: u64| (ms / u64::from(FRAME_MS)) as u32;
    // The port: every seed, the steps that moved the party. A blocked step is left out: it runs
    // a world turn of one pick (its setup and one pass), in the original as in the port
    // (tools/oracle/trace.mjs, re/notes/oracle.md §E), not a step's.
    let (mut all, mut first5) = (Vec::new(), Vec::new());
    let mut spent: BTreeMap<&str, (u32, u64)> = BTreeMap::new();
    for seed in SEEDS {
        let turns = play(&files, seed, HOST_STEP_MS, false);
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
    // Spec 03 §1: the same steps take the same engine time at the hosts' step (spec 06) and at
    // ticks of half of it, and at 16 ms and 8 ms ticks; polls with no time elapsed add nothing
    // (to within a host tick per step). The finer tick divides the coarser one: where it does
    // not (20 against 8), the key's tick and the end's rounding differ by more than a tick.
    let Some(files) = files() else {
        return;
    };
    for (coarse, finer) in [(HOST_STEP_MS, HOST_STEP_MS / 2), (16, 8)] {
        for seed in [0, 5] {
            let base: Vec<u64> = play(&files, seed, coarse, false)
                .iter()
                .map(|t| t.ms)
                .collect();
            for (tick, zero) in [(finer, false), (coarse, true)] {
                let other: Vec<u64> = play(&files, seed, tick, zero)
                    .iter()
                    .map(|t| t.ms)
                    .collect();
                println!(
                    "seed {seed}: ms per step at {coarse} ms ticks {base:?}, at {tick} ms \
                     (zero polls {zero}) {other:?}"
                );
                // Each step ends within its own run's tick of the same deadline.
                for (a, b) in base.iter().zip(&other) {
                    assert!(
                        b + u64::from(coarse) >= *a && *b <= a + u64::from(tick),
                        "seed {seed}: {base:?} vs {other:?}"
                    );
                }
            }
        }
    }
}

/// The original in the castle without the fight (tools/oracle/fight.mjs with U6_FIGHT_SETUP=town:
/// the start's assault actors passed over, combat broken off, the steps of [`TOWN_KEYS`]): PAL
/// frames from the key taken to the next prompt.
#[cfg(not(feature = "wide"))]
const ORACLE_TOWN: [u32; 7] = [28, 43, 33, 48, 32, 41, 32];

/// The steps of the oracle's town runs: south along the carpet and back.
#[cfg(not(feature = "wide"))]
const TOWN_KEYS: [Key; 7] = [
    Key::Down,
    Key::Down,
    Key::Down,
    Key::Down,
    Key::Down,
    Key::Up,
    Key::Up,
];

// A comparison with the original's run: Original only (spec 13 Tests).
#[cfg(not(feature = "wide"))]
#[test]
fn the_castle_without_the_fight_takes_the_originals_time() {
    let Some(files) = files() else {
        return;
    };
    let frames = |ms: u64| (ms / u64::from(FRAME_MS)) as u32;
    let mut spent: BTreeMap<&str, (u32, u64)> = BTreeMap::new();
    let mut all = Vec::new();
    for seed in SEEDS {
        let turns = play_keys(&files, seed, &TOWN_KEYS, true, HOST_STEP_MS, false);
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

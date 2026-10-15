//! Save and restore on the user's files: the archive.org zip (skipped when absent) and the
//! original's own save taken in the oracle (`~/Games/ultima6/oracle/saveload/`, skipped when
//! absent). Reads them, copies nothing; prints observations only.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use u6_formats::objlist::ObjList;
use u6_testkit::assert_same;

use super::objlist::{OBJLIST_LEN, TABLES, decode, encode};
use super::*;
use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::objects::{ACTOR_SLOTS, BLOCK_COUNT, FIRST_TEMP_ACTOR};
use crate::state::State;
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
const ORACLE: &str = "/Users/zhugehyuk/Games/ultima6/oracle/saveload";

fn zip_files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// `ORACLE/<dir>/*` as `savegame/<name>` (a `savegame/` directory taken off the PLAYER disk).
fn oracle_save(dir: &str) -> Option<MemStore> {
    let path = Path::new(ORACLE).join(dir);
    let Ok(entries) = std::fs::read_dir(&path) else {
        eprintln!("skipped: {} not present", path.display());
        return None;
    };
    let mut files = BTreeMap::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        files.insert(format!("savegame/{name}"), std::fs::read(e.path()).quiet());
    }
    Some(MemStore { files })
}

/// What the test asks the game to do at its next poll.
enum Cmd {
    Save,
    Restore,
}

/// The real game future (`main_loop::game_first`) with commands run at the start of a poll, inside
/// the engine's poll (so code that reaches the current `Io` finds it), while the loop is
/// suspended at its command-key wait.
struct Inject {
    game: Pin<Box<dyn Future<Output = ()>>>,
    cmds: Rc<RefCell<VecDeque<Cmd>>>,
    state: Rc<RefCell<State>>,
    io: Rc<Io>,
}

impl Future for Inject {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        loop {
            let cmd = self.cmds.borrow_mut().pop_front();
            let Some(cmd) = cmd else { break };
            let mut s = self.state.borrow_mut();
            match cmd {
                Cmd::Save => save(&mut s),
                Cmd::Restore => restore(&mut s, &self.io).quiet(),
            }
        }
        self.game.as_mut().poll(cx)
    }
}

struct Run {
    e: Engine,
    cmds: Rc<RefCell<VecDeque<Cmd>>>,
}

impl Run {
    /// Boots `files` as `Engine::game_first` does (seed 0) and runs to the first command prompt.
    fn boot(files: Files) -> Run {
        let mut state = State::new_game(&files).quiet();
        state.rng.seed(0);
        let cmds = Rc::new(RefCell::new(VecDeque::new()));
        let c = Rc::clone(&cmds);
        let e = Engine::with_game(state, files, move |st, io| Inject {
            game: Box::pin(crate::main_loop::game_first(Rc::clone(&st), Rc::clone(&io))),
            cmds: c,
            state: st,
            io,
        });
        let mut run = Run { e, cmds };
        run.settle();
        assert!(run.at_prompt(), "boot reaches the command prompt");
        run
    }

    /// The command loop waits for a command key and nothing is queued.
    fn at_prompt(&self) -> bool {
        self.e.io().pending_events() == 0 && self.e.state().main.reading == 1
    }

    fn settle(&mut self) {
        for _ in 0..40 {
            self.e.tick(&[], 16);
        }
    }

    fn press(&mut self, k: Key) {
        self.e.tick(&[InputEvent::Key(k)], 16);
        self.settle();
    }

    fn command(&mut self, cmd: Cmd) {
        assert!(self.at_prompt());
        self.cmds.borrow_mut().push_back(cmd);
        self.settle();
    }

    fn state(&self) -> State {
        self.e.state().clone()
    }
}

/// What a Save of `s` would write: objlist and all 69 block files (on a copy).
fn canon(s: &State) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut c = s.clone();
    save(&mut c);
    let blocks = c.objects.blocks.iter().map(|(_, b)| b.to_bytes()).collect();
    (c.saveload.objlist.quiet(), blocks)
}

/// Differing objlist bytes per table: (table, number of bytes, first indices within it).
fn table_diffs(a: &[u8], b: &[u8]) -> Vec<(&'static str, usize, Vec<usize>)> {
    let mut out = Vec::new();
    for (off, len, name) in TABLES {
        let idx: Vec<usize> = (0..len)
            .filter(|&i| a.get(off + i) != b.get(off + i))
            .collect();
        if !idx.is_empty() {
            out.push((name, idx.len(), idx.into_iter().take(8).collect()));
        }
    }
    out
}

/// Records of block file `bytes` (8 bytes each, after the count).
fn records(bytes: &[u8]) -> Vec<[u8; 8]> {
    bytes[2..]
        .chunks_exact(8)
        .map(|r| r.try_into().quiet())
        .collect()
}

/// Block indices whose files differ, with the record counts.
fn block_diffs(a: &[Vec<u8>], b: &[Vec<u8>]) -> Vec<(String, usize, usize)> {
    (0..BLOCK_COUNT)
        .filter(|&i| a[i] != b[i])
        .map(|i| (block_file(i), records(&a[i]).len(), records(&b[i]).len()))
        .collect()
}

#[test]
fn decode_agrees_with_the_formats_parser() {
    let Some(files) = zip_files() else { return };
    let raw = files.get(OBJLIST_FILE).quiet();
    let list = ObjList::parse(raw).quiet();
    let mut s = State::empty();
    decode(&mut s, raw);
    let (o, a) = (&s.objects, &s.actors);
    for (i, r) in list.actors.iter().enumerate() {
        assert_same!([o.obj_status[i]], [r.obj_flags], "the flags of actor {i}");
        assert!(o.location[i] == r.pos, "the location of actor {i}");
        let tf = [o.type_frame[i] & 0x3ff, o.type_frame[i] >> 10];
        let want = [r.obj_n, u16::from(r.frame)];
        assert_same!(tf, want, "the type and frame of actor {i}");
        assert_same!(
            [o.qq[i]],
            [r.unknown_0600],
            "the word at 0x600 of actor {i}"
        );
        let t = &a.attrs;
        let got = [
            t.status[i],
            t.strength[i],
            t.dexterity[i],
            t.intelligence[i],
            t.hp[i],
            t.level[i],
            t.unknown_10f1[i],
            a.worktype[i],
            t.combat_mode[i],
            t.magic[i],
            a.movement_points[i],
            t.talk_flags[i],
            t.movement_flags[i],
        ];
        let want = [
            r.status,
            r.strength,
            r.dexterity,
            r.intelligence,
            r.hp,
            r.level,
            r.unknown_10f1,
            r.worktype,
            r.combat_mode,
            r.magic,
            r.movement_points,
            r.talk_flags,
            r.movement_flags,
        ];
        assert_same!(got, want, "the attributes of actor {i}");
        assert_same!([t.exp[i]], [r.exp], "the experience of actor {i}");
        let base = [t.base_type_frame[i] & 0x3ff];
        assert_same!(base, [r.base_obj_n], "the base type of actor {i}");
        let bytes = [t.unknown_18f1[i], t.unknown_1af1[i]];
        let want = [raw[0x18F1 + i], raw[0x1AF1 + i]];
        assert_same!(bytes, want, "the bytes at 0x18F1 and 0x1AF1 of actor {i}");
    }
    assert_same!(a.party.names, list.party.names, "the party names");
    assert_same!(a.party.members, list.party.roster, "the roster");
    assert_same!([a.party.size], [list.party.size], "the party size");
    let c = list.clock;
    let k = &a.calendar;
    let clock = [k.minute, k.hour, k.day, k.month].map(u16::from);
    let want = [c.minute, c.hour, c.day, c.month].map(u16::from);
    assert_same!(clock, want, "the clock");
    assert_same!([k.year], [c.year], "the year");
    let p = list.player;
    assert_same!(
        [a.quest_flag, a.rest_counter, a.karma, a.wind],
        [p.quest_flag, p.rest_counter, p.karma, p.wind_direction],
        "the player bytes"
    );
    assert_same!(a.timers, list.timers, "the timers");
    assert_same!(a.objlist_tail, raw[0x1BF1..0x1C73], "the objlist tail");
    // The view centre is not decoded (0x105350 takes it from the leader), so a state that
    // holds the file's centre words writes the file's first 0x1C73 bytes back.
    let t = &raw[0x1BF1 + 0x0c..0x1BF1 + 0x12];
    let w = |i: usize| u16::from_be_bytes([t[i], t[i + 1]]);
    s.map.centre = u6_formats::position::Position {
        x: w(0),
        y: w(2),
        z: w(4) as u8,
    };
    assert_same!(encode(&s), raw[..OBJLIST_LEN], "the objlist written back");
}

/// The surface block holding the leader's superchunk (128 × 128 tiles a block, x then y): in a
/// start save the block of the opening fight (the egg the party stands by).
fn leader_block(s: &State) -> usize {
    let at = s.leader_position().expect("a leader");
    usize::from(at.y / 128) * 8 + usize::from(at.x / 128)
}

/// The shipped party roster: the actor numbers at objlist 0x0FE0, as many as the size at 0x0FF0
/// (formats.md §6), read from the raw file.
fn roster(files: &Files) -> Vec<usize> {
    let raw = files.get(OBJLIST_FILE).quiet();
    let n = usize::from(raw[0x0FF0]).min(16);
    raw[0x0FE0..0x0FE0 + n]
        .iter()
        .map(|&a| usize::from(a))
        .collect()
}

/// The new-game files the port boots from: objlist and the 69 blocks.
fn shipped(files: &Files) -> (Vec<u8>, Vec<Vec<u8>>) {
    let objlist = files.get(OBJLIST_FILE).quiet()[..OBJLIST_LEN].to_vec();
    let blocks = (0..BLOCK_COUNT)
        .map(|i| files.get(&block_file(i)).quiet().to_vec())
        .collect();
    (objlist, blocks)
}

/// Why the boot changes a shipped objlist byte before any command, in the port and the
/// original alike: 0x1217d4 raises a dexterity of 0 to 1 and takes actor 0 out of a schedule
/// worktype; 0x105350 puts the view centre (block 0x0c..0x11) on the leader; the clock tick at
/// +0 (0x10368a) sets the moons (block 0x7c..0x7f); idle animation turns actors; the opening
/// fight (the egg 0x1319b2 hatches its creatures into temporary actor slots, combat starts
/// 0x121c64; [`opening_fight`]). The portrait byte (block 0x81) is the port's character-creation
/// stub.
fn boot_change(table: &str, i: usize, x: u8, y: u8, party: &[usize]) -> Option<&'static str> {
    if opening_fight(table, i, party) {
        return Some("0x1319b2 / 0x121c64 opening fight");
    }
    Some(match table {
        "A4+0x400c dexterity" if x == 0 && y == 1 => "0x1217d4 dexterity at least 1",
        "0x139188 worktype" if i == 0 => "0x1217d4 actor 0 worktype",
        "A4-0x7b84 block" if (0x0c..0x12).contains(&i) => "0x105350 view centre",
        "A4-0x7b84 block" if (0x7c..0x80).contains(&i) => "0x10368a moons",
        "A4-0x7b84 block" if i == 0x81 => "character-creation stub",
        _ if frame_only(table, i, x, y) => "idle frame",
        _ => return None,
    })
}

#[test]
fn a_save_of_the_start_state_writes_the_shipped_bytes_back() {
    let Some(files) = zip_files() else { return };
    let (old_list, old_blocks) = shipped(&files);
    let party = roster(&files);
    let mut run = Run::boot(files);
    run.command(Cmd::Save);
    let s = run.state();
    let new_list = s.saveload.objlist.clone().quiet();
    let new_blocks: Vec<Vec<u8>> = s.objects.blocks.iter().map(|(_, b)| b.to_bytes()).collect();
    let mut why = BTreeMap::new();
    for (off, len, name) in TABLES {
        for i in 0..len {
            let (x, y) = (old_list[off + i], new_list[off + i]);
            if x != y {
                let w = boot_change(name, i, x, y, &party);
                assert!(w.is_some(), "{name} byte {i:#x} changed, unexplained");
                *why.entry(w.quiet()).or_insert(0) += 1;
            }
        }
    }
    let reasons: Vec<&str> = why.keys().copied().collect();
    eprintln!("objlist bytes changed by the boot, for: {reasons:?}");
    // The Save commits the blocks of the region: the 2 × 2 surface blocks the window spans,
    // the leader's among them.
    let committed: Vec<usize> = (0..BLOCK_COUNT)
        .filter(|&i| s.saveload.block_gen[i] == 1)
        .collect();
    let fight = leader_block(&s);
    let (xs, ys): (Vec<usize>, Vec<usize>) = committed.iter().map(|&i| (i % 8, i / 8)).unzip();
    let (x0, y0) = (*xs.iter().min().quiet(), *ys.iter().min().quiet());
    let square: Vec<usize> = [(0, 0), (1, 0), (0, 1), (1, 1)]
        .iter()
        .map(|(dx, dy)| (y0 + dy) * 8 + x0 + dx)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert!(
        committed.iter().all(|&i| i < 64) && committed == square,
        "the committed blocks are not a 2 x 2 square of the surface"
    );
    assert!(committed.contains(&fight), "the leader's block committed");
    for (name, _, _) in block_diffs(&old_blocks, &new_blocks) {
        let i = (0..BLOCK_COUNT).find(|&i| block_file(i) == name).quiet();
        let (mut ra, mut rb) = (records(&old_blocks[i]), records(&new_blocks[i]));
        if i == fight {
            // The egg hatched (its status) and armed its creatures (their gear).
            assert!(gear_records(&rb) > 0, "the hatched creatures' gear");
            assert_same!(
                strip_fight(ra),
                strip_fight(rb),
                "{name}: the records apart from the fight"
            );
            continue;
        }
        eprintln!("{name}: the same records in another order");
        ra.sort();
        rb.sort();
        assert_same!(ra, rb, "{name}: the same records");
        assert!(committed.contains(&i), "{name}: a block of the region");
    }
}

/// Prints how `got` differs from `want` (objlist tables, tail bytes, block records); returns
/// the number of differing block files.
///
/// Content-free: table names, byte and record indices only (no counts of the user's records).
fn report(what: &str, want: &(Vec<u8>, Vec<Vec<u8>>), got: &(Vec<u8>, Vec<Vec<u8>>)) -> usize {
    for (table, _, first) in table_diffs(&want.0, &got.0) {
        eprintln!("{what}: objlist {table} differs, first at {first:x?}");
    }
    let tail: Vec<usize> = (0..0x82)
        .filter(|&i| want.0.get(0x1BF1 + i) != got.0.get(0x1BF1 + i))
        .collect();
    if !tail.is_empty() {
        eprintln!("{what}: tail bytes differ at {tail:x?}");
    }
    let diffs = block_diffs(&want.1, &got.1);
    for (name, a, b) in &diffs {
        let i = (0..BLOCK_COUNT).find(|&i| &block_file(i) == name).quiet();
        let (ra, rb) = (records(&want.1[i]), records(&got.1[i]));
        let (mut sa, mut sb) = (ra.clone(), rb.clone());
        sa.sort();
        sb.sort();
        let same_count = a == b;
        eprintln!(
            "{what}: {name} differs; the same record count {same_count}, the same set {}",
            sa == sb
        );
        for (n, (x, y)) in ra
            .iter()
            .zip(&rb)
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .take(6)
        {
            let bytes: Vec<usize> = (0..8).filter(|&k| x[k] != y[k]).collect();
            eprintln!("    record #{n}: bytes {bytes:?} differ");
        }
    }
    diffs.len()
}

/// The `savegame/` files of a store, objlist and the 69 blocks.
fn store_files(store: &MemStore) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut f = Files::default();
    f.overlay("s", store.files.clone());
    let objlist = f.get(OBJLIST_FILE).quiet().to_vec();
    let blocks = (0..BLOCK_COUNT)
        .map(|i| f.get(&block_file(i)).quiet().to_vec())
        .collect();
    (objlist, blocks)
}

/// A record with its container-content byte 3 cleared: 0x10d376 packs a container content's
/// record index through a 3-byte stack buffer whose third byte it never writes (A5-1), so the
/// original leaves stack contents there; nothing reads it back.
fn defined(r: &[u8; 8]) -> [u8; 8] {
    let mut r = *r;
    if r[0] & 0x18 == 0x08 {
        r[3] = 0;
    }
    r
}

fn defined_block(bytes: &[u8]) -> Vec<[u8; 8]> {
    records(bytes).iter().map(defined).collect()
}

/// An actor's frame bits: the high 6 bits of its type + frame word (objlist 0x0400, even
/// bytes). Idle animation turns actors as display time passes (0x10202e, A4-0x7afa); the
/// oracle saved after ~1000 frames, the port after 40 ticks.
fn frame_only(table: &str, index: usize, x: u8, y: u8) -> bool {
    table == "0x137988 type + frame" && index.is_multiple_of(2) && x & 3 == y & 3
}

/// The opening fight in a start save: the egg by the party (0x1319b2) hatches its creatures
/// into the temporary actor slots (0x131646 from `FIRST_TEMP_ACTOR` on), and combat mode starts
/// for the objlist's combat byte (0x121c64: each member's worktype from its combat mode,
/// worktype operand = its party index, the solo worktype kept at A4-0x7b0a). The port runs both
/// since milestone 2 but from its own RNG state (seed 0; the oracle's differs), so the
/// creatures' stats, places and gear differ from the original's. `party` = the roster.
fn opening_fight(table: &str, index: usize, party: &[usize]) -> bool {
    let actor = match table {
        "0x135588 location" => index / 3,
        "0x137988 type + frame"
        | "A4+0x250c quality / quantity"
        | "A4+0x440c experience"
        | "0x139688 base type + frame" => index / 2,
        _ => index,
    };
    temporary(actor)
        || (table == "0x139188 worktype" && party.contains(&actor))
        || (table == "0x139488 worktype operand" && party.contains(&actor))
        || (table == "A4-0x7b84 block" && index == 0x7a)
}

/// A temporary actor slot (0xe0..0x100, the monsters' slots).
fn temporary(actor: usize) -> bool {
    (FIRST_TEMP_ACTOR as usize..ACTOR_SLOTS).contains(&actor)
}

/// A block record of the opening fight: a readied object (the gear 0x13121a gives) of a
/// temporary actor, or an egg (type 0x14f), whose status the hatch changes.
fn fight_record(r: &[u8; 8]) -> bool {
    (r[0] & 0x38 == 0x38 && temporary(usize::from(r[1]))) || r[4..6] == [1, 0x4f]
}

/// The records of a block without the opening fight's, container indices (which shift after
/// them) cleared, sorted.
fn strip_fight(v: Vec<[u8; 8]>) -> Vec<[u8; 8]> {
    let mut v: Vec<[u8; 8]> = v
        .into_iter()
        .filter(|r| !fight_record(r))
        .map(|mut r| {
            if r[0] & 0x18 == 0x08 {
                r[1] = 0;
                r[2] = 0;
            }
            r
        })
        .collect();
    v.sort();
    v
}

/// The hatched creatures' gear records (readied, owned by a temporary actor) of a block.
fn gear_records(v: &[[u8; 8]]) -> usize {
    v.iter()
        .filter(|r| r[0] & 0x38 == 0x38 && temporary(usize::from(r[1])))
        .count()
}

#[test]
fn the_ports_start_save_against_the_originals() {
    let Some(mut files) = zip_files() else { return };
    let (Some(boot), Some(saved)) = (oracle_save("start-boot"), oracle_save("start-saved")) else {
        return;
    };
    attach(&mut files, &boot).quiet();
    let party = roster(&files);
    let mut run = Run::boot(files);
    run.command(Cmd::Save);
    let s = run.state();
    let fight = leader_block(&s);
    let mut got_store = boot.clone();
    got_store.store(&changed_since(&s, 0)).quiet();
    let want = store_files(&saved);
    let got = store_files(&got_store);
    assert!(got.0.len() == want.0.len(), "objlist length");
    report("port vs original", &want, &got);
    for (off, len, name) in TABLES {
        for i in 0..len {
            let (x, y) = (want.0[off + i], got.0[off + i]);
            if x != y {
                let why = frame_only(name, i, x, y) || opening_fight(name, i, &party);
                assert!(why, "{name} byte {i:#x} unexplained");
            }
        }
    }
    for i in 0..BLOCK_COUNT {
        let (w, g) = (defined_block(&want.1[i]), defined_block(&got.1[i]));
        if i != fight {
            assert_same!(w, g, "the records of {}", block_file(i));
            continue;
        }
        // The leader's block: the hatched creatures' readied gear (owned by temporary actors)
        // and the egg's status come from the RNG (the port's seed is not the oracle's), and the
        // container indices after them shift; both sides arm their creatures.
        let (wg, gg) = (gear_records(&w), gear_records(&g));
        assert!(
            wg > 0 && gg > 0,
            "both sides' creatures carry gear (0x13121a)"
        );
        assert_same!(
            strip_fight(w),
            strip_fight(g),
            "the records of the leader's block apart from the fight"
        );
    }
}

/// The boot's own path (`State::new_game`, then game's main) of the user's files under a save
/// whose party-size byte (objlist 0x0FF0) is 17: clamped to 15, the boot reaches the prompt, and
/// a pass turn and an hour of the clock run over the 15 members.
#[test]
fn a_saved_party_size_past_the_roster_boots_clamped() {
    let Some(mut files) = zip_files() else { return };
    let mut f = files.get(OBJLIST_FILE).quiet().to_vec();
    f[0x0FF0] = 17;
    let store = MemStore {
        files: BTreeMap::from([(OBJLIST_FILE.to_string(), f)]),
    };
    attach(&mut files, &store).quiet();
    let mut run = Run::boot(files);
    assert!(
        run.e.state().actors.party.size == 15,
        "clamped at the vessel slot"
    );
    let hour = run.e.state().actors.calendar.hour;
    run.press(Key::Space);
    assert!(run.at_prompt(), "a pass turn comes back to the prompt");
    let mut s = run.state();
    crate::actors::clock::tick(&mut s, 60);
    assert!(s.actors.calendar.hour == (hour + 1) % 24, "an hour on");
}

/// The original's own save boots in the port, and the port's save of it is the original's
/// save of the same state (the original booted from that save and saved again at once).
#[test]
fn the_originals_save_loads_and_saves_like_the_original() {
    let Some(mut files) = zip_files() else { return };
    let (Some(saved), Some(resaved)) = (oracle_save("start-saved"), oracle_save("resaved")) else {
        return;
    };
    let raw = &saved.files[OBJLIST_FILE];
    assert!(raw.len() == OBJLIST_LEN);
    // The temporary actors the save holds: a type in the low 10 bits of the BE type word at
    // objlist 0x400 + 2 * actor (formats.md §6), read from the saved file.
    let held: Vec<usize> = (FIRST_TEMP_ACTOR as usize..ACTOR_SLOTS)
        .filter(|&a| u16::from_be_bytes([raw[0x400 + 2 * a], raw[0x401 + 2 * a]]) & 0x3ff != 0)
        .collect();
    assert!(!held.is_empty(), "the original saved its hatched creatures");
    attach(&mut files, &saved).quiet();
    let mut run = Run::boot(files);
    {
        let s = run.e.state();
        let back = held
            .iter()
            .filter(|&&a| s.objects.type_frame[a] & 0x3ff != 0)
            .count();
        assert!(back == held.len(), "the saved creatures are back");
    }
    run.command(Cmd::Save);
    let s = run.state();
    let mut got_store = saved.clone();
    got_store.store(&changed_since(&s, 0)).quiet();
    let want = store_files(&resaved);
    let got = store_files(&got_store);
    report("port re-save vs original re-save", &want, &got);
    for (off, len, name) in TABLES {
        for i in 0..len {
            let (x, y) = (want.0[off + i], got.0[off + i]);
            assert!(
                x == y || frame_only(name, i, x, y),
                "{name} byte {i:#x} differs beyond the frame bits"
            );
        }
    }
    for i in 0..BLOCK_COUNT {
        let w = defined_block(&want.1[i]);
        let g = defined_block(&got.1[i]);
        assert_same!(w, g, "the records of {}", block_file(i));
    }
    // The original moves records on every load + save cycle (0x10db82's merge, 0x10da94);
    // the port reproduces those moves.
    let before = store_files(&saved);
    let moved: Vec<String> = (0..BLOCK_COUNT)
        .filter(|&i| before.1[i] != want.1[i])
        .map(block_file)
        .collect();
    eprintln!("blocks the original's second save rewrote: {moved:?}");
    assert!(!moved.is_empty());
}

/// The scroll lines so far plus the line being written.
fn transcript(s: &State) -> Vec<String> {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let mut last: String = t.transcript_line.iter().map(|&b| char::from(b)).collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| char::from(b)));
    lines.push(last);
    lines
}

impl Run {
    fn leader_at(&self) -> u6_formats::position::Position {
        self.e.state().leader_position().quiet()
    }

    /// Presses arrows until the party has stepped `n` times (a blocked direction is one turn;
    /// then the next of S, E, W, N is tried). Returns the keys pressed.
    fn walk(&mut self, n: usize) -> Vec<Key> {
        let mut pressed = Vec::new();
        let dirs = [Key::Down, Key::Right, Key::Left, Key::Up];
        let (mut d, mut moved) = (0, 0);
        while moved < n {
            assert!(
                pressed.len() < 4 * n + 8,
                "the party cannot walk: {pressed:?}"
            );
            let before = self.leader_at();
            self.press(dirs[d]);
            pressed.push(dirs[d]);
            if self.leader_at() != before {
                moved += 1;
            } else {
                d = (d + 1) % dirs.len();
            }
            assert!(self.at_prompt(), "a step ends at the prompt");
        }
        pressed
    }

    /// `T`, the cross-hair onto the nearest actor not in the party within 5 tiles (else the
    /// nearest member), Return; then answers the conversation: "bye" at a line input, else
    /// space / N / Y until the prompt is back. Returns the actor talked to.
    fn talk(&mut self) -> u16 {
        let (target, dx, dy) = {
            let s = self.e.state();
            let c = s.leader_position().quiet();
            let leader = s.actors.party.leader().quiet().0;
            let party: Vec<u16> = s.actors.party.active().map(|a| a.0).collect();
            let mut best: Option<(bool, i32, u16, i32, i32)> = None;
            for a in 1..0xe0u16 {
                let p = s.objects.location[usize::from(a)];
                if a == leader || s.objects.type_frame[usize::from(a)] == 0 || p.z != c.z {
                    continue;
                }
                let (dx, dy) = (
                    i32::from(p.x) - i32::from(c.x),
                    i32::from(p.y) - i32::from(c.y),
                );
                if dx.abs() > 5 || dy.abs() > 5 {
                    continue;
                }
                let key = (party.contains(&a), dx.abs().max(dy.abs()), a, dx, dy);
                if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
                    best = Some(key);
                }
            }
            let b = best.expect("an actor near the party");
            (b.2, b.3, b.4)
        };
        self.press(Key::Char('t'));
        let (h, v) = (
            if dx < 0 { Key::Left } else { Key::Right },
            if dy < 0 { Key::Up } else { Key::Down },
        );
        for _ in 0..dx.abs() {
            self.press(h);
        }
        for _ in 0..dy.abs() {
            self.press(v);
        }
        self.press(Key::Enter);
        let mut said = false;
        for i in 0..200 {
            if self.at_prompt() {
                break;
            }
            if self.e.state().ui.input.line_input {
                for c in "bye".chars() {
                    self.e.tick(&[InputEvent::Key(Key::Char(c))], 16);
                }
                self.press(Key::Enter);
                said = true;
            } else {
                self.press([Key::Space, Key::Char('n'), Key::Char('y')][i % 3]);
            }
        }
        assert!(self.at_prompt(), "the conversation ended");
        assert!(said, "the conversation asked for a line");
        target
    }
}

/// The parts of two states that differ, named by field (actor tables compared per table).
fn state_diffs(a: &State, b: &State) -> Vec<String> {
    let mut out = Vec::new();
    let mut check = |name: &str, same: bool| {
        if !same {
            out.push(name.to_string());
        }
    };
    let n = 0x100;
    let (oa, ob) = (&a.objects, &b.objects);
    check(
        "objects.obj_status[actors]",
        oa.obj_status[..n] == ob.obj_status[..n],
    );
    check(
        "objects.location[actors]",
        oa.location[..n] == ob.location[..n],
    );
    check(
        "objects.type_frame[actors]",
        oa.type_frame[..n] == ob.type_frame[..n],
    );
    check("objects.qq[actors]", oa.qq[..n] == ob.qq[..n]);
    check("objects.next (slot numbering)", oa.next == ob.next);
    check(
        "objects.type_frame[objects]",
        oa.type_frame[n..] == ob.type_frame[n..],
    );
    check("objects.free", oa.free == ob.free);
    check(
        "objects.loaded_surface",
        oa.loaded_surface == ob.loaded_surface,
    );
    check("objects.blocks", oa.blocks == ob.blocks);
    check("objects.tmp_flag", oa.tmp_flag == ob.tmp_flag);
    check("objects.save_mode", oa.save_mode == ob.save_mode);
    let (aa, ab) = (&a.actors, &b.actors);
    check("actors.attrs", aa.attrs == ab.attrs);
    check(
        "actors.movement_points",
        aa.movement_points == ab.movement_points,
    );
    check("actors.worktype", aa.worktype == ab.worktype);
    check("actors.schedules", aa.schedules == ab.schedules);
    check("actors.party", aa.party == ab.party);
    check("actors.calendar", aa.calendar == ab.calendar);
    check("actors.quest_flag", aa.quest_flag == ab.quest_flag);
    check(
        "actors.quest_flag_copy",
        aa.quest_flag_copy == ab.quest_flag_copy,
    );
    check("actors.timers", aa.timers == ab.timers);
    check(
        "actors.karma/wind/rest",
        (aa.karma, aa.wind, aa.rest_counter) == (ab.karma, ab.wind, ab.rest_counter),
    );
    let tail: Vec<usize> = (0..0x82)
        .filter(|&i| aa.objlist_tail[i] != ab.objlist_tail[i])
        .collect();
    check(&format!("actors.objlist_tail {tail:x?}"), tail.is_empty());
    let fields = work_diffs(&aa.work, &ab.work);
    check(&format!("actors.work {fields:?}"), fields.is_empty());
    check("map.centre", a.map.centre == b.map.centre);
    check("map.view_flags", a.map.view_flags == b.map.view_flags);
    check("map.view_tiles", a.map.view_tiles == b.map.view_tiles);
    check("map.anim_enabled", a.map.anim_enabled == b.map.anim_enabled);
    check("tables", a.tables == b.tables);
    check("ui.panel", a.ui.panel == b.ui.panel);
    check("ui.messages", a.ui.messages == b.ui.messages);
    check("converse", a.converse == b.converse);
    check("gfx", a.gfx == b.gfx);
    check("main", a.main == b.main);
    check("rng", a.rng == b.rng);
    out
}

/// The names of the [`crate::actors::ActorWork`] fields that differ, never their values. The
/// pattern names every field, so a field added to the struct breaks the build here until it is
/// compared too.
fn work_diffs(a: &crate::actors::ActorWork, b: &crate::actors::ActorWork) -> Vec<&'static str> {
    macro_rules! differing {
        ($($field:ident),+ $(,)?) => {{
            let crate::actors::ActorWork { $($field: _),+ } = a;
            let mut out = Vec::new();
            $(if a.$field != b.$field {
                out.push(stringify!($field));
            })+
            out
        }};
    }
    differing!(
        prompt,
        time_stop_over,
        last_hour,
        bell_strikes,
        hour_strikes,
        saw_damage,
        avoid_damage,
        stepped,
        teleports,
        searches,
        force_teleport,
        swap_ok,
        hic,
        slot,
        leader_kept,
        walk_through_damage,
        party_centre,
        enemy_centre,
        front,
        enemies,
        flag_6a88,
        follow,
        step_blocked,
        mouse_walk,
        mouse_target,
        resurrect_pending,
    )
}

/// Objlist bytes that differ outside the actors' frame bits (the high 6 bits of each
/// type + frame word: idle animation turns actors as display time passes, 0x10202e) and the
/// hit mark (movement flag 0x08: a hit's flash 0x1230ce sets it, every clock tick 0x10368a
/// clears it, and a boot or a restore runs the clock at +0).
fn objlist_diffs_but_frames(a: &[u8], b: &[u8]) -> Vec<(&'static str, usize)> {
    let mut out = Vec::new();
    for (off, len, name) in TABLES {
        for i in 0..len {
            let (mut x, mut y) = (a[off + i], b[off + i]);
            if name == "0x139588 movement flags" {
                (x, y) = (x & !0x08, y & !0x08);
            }
            let frame_byte = name == "0x137988 type + frame" && i.is_multiple_of(2);
            if (frame_byte && x & 3 != y & 3) || (!frame_byte && x != y) {
                out.push((name, i));
            }
        }
    }
    out
}

#[test]
fn walk_talk_save_restore() {
    let Some(files) = zip_files() else { return };
    let mut run = Run::boot(files.clone());
    let start = run.leader_at();
    let keys = run.walk(10);
    let walked = run.leader_at();
    let npc = run.talk();
    let lines = transcript(&run.e.state());
    eprintln!(
        "walked with {} keys (moved {}); talked to actor slot {npc:#x}",
        keys.len(),
        walked != start
    );
    assert!(
        lines.iter().any(|l| l.contains("bye")),
        "no line holds the typed bye"
    );
    run.command(Cmd::Save);
    let saved = run.state();
    assert!(saved.objects.tmp_flag.iter().all(|&f| f == 0));
    let committed: Vec<String> = (0..BLOCK_COUNT)
        .filter(|&i| saved.saveload.block_gen[i] == 1)
        .map(block_file)
        .collect();
    eprintln!("save 1 committed {committed:?}");
    assert!(
        committed.len() >= 4,
        "the region's four superchunks at least"
    );
    let want = canon(&saved);
    let written = saved.saveload.objlist.clone().quiet();
    let tables: Vec<&str> = table_diffs(&written, &want.0).iter().map(|d| d.0).collect();
    eprintln!("re-save after save: tables differing {tables:?}");

    // 1. A fresh engine booted from the stored save.
    let mut store = MemStore::default();
    let mut sync = SaveSync::default();
    assert!(sync.flush(&run.e, &mut store).quiet() == 1 + committed.len());
    assert!(sync.flush(&run.e, &mut store).quiet() == 0, "nothing new");
    let mut f = files.clone();
    attach(&mut f, &store).quiet();
    let fresh = Run::boot(f);
    let got = canon(&fresh.state());
    let diffs = objlist_diffs_but_frames(&want.0, &got.0);
    eprintln!("fresh engine: objlist bytes differing beyond actor frames: {diffs:x?}");
    eprintln!(
        "fresh engine: state parts differing: {:?}",
        state_diffs(&saved, &fresh.state())
    );
    assert!(diffs.is_empty());
    assert!(
        block_diffs(&want.1, &got.1).is_empty(),
        "fresh engine: block files"
    );
    assert!(fresh.leader_at() == walked);

    // 2. Restore in the same engine after walking on.
    run.walk(3);
    assert!(run.leader_at() != walked);
    run.command(Cmd::Restore);
    let restored = run.state();
    assert!(restored.objects.save_mode == 0x20, "save mode play");
    assert!(run.leader_at() == walked);
    let got = canon(&restored);
    let diffs = objlist_diffs_but_frames(&want.0, &got.0);
    eprintln!("restore: objlist bytes differing beyond actor frames: {diffs:x?}");
    eprintln!(
        "restore: state parts differing: {:?}",
        state_diffs(&saved, &restored)
    );
    assert!(diffs.is_empty());
    assert!(
        block_diffs(&want.1, &got.1).is_empty(),
        "restore: block files"
    );
    assert!(run.at_prompt());
}

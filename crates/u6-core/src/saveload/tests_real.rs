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
use u6_formats::position::Position;
use u6_testkit::assert_same;

use super::objlist::{OBJLIST_LEN, TABLES, decode, encode};
use super::*;
use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::objects::{ACTOR_SLOTS, BLOCK_COUNT, FIRST_TEMP_ACTOR, Slot};
use crate::state::State;
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
#[cfg(not(feature = "wide"))]
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
#[cfg(not(feature = "wide"))]
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
    /// The egg on this tile made unhatched (status bit 0x40 cleared) and full (quantity 100), so
    /// its next offer hatches for sure: 0x1319b2 draws 1..=100 against the quantity
    /// (`objects/commands/eggs.rs` `hatch_egg`).
    Rearm(Position),
    /// A change to the state, as the objects API makes it (2026-10-25: a spellbook given).
    Edit(fn(&mut State)),
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
                Cmd::Edit(edit) => edit(&mut s),
                Cmd::Rearm(at) => {
                    let o = &mut s.objects;
                    for e in 0x100..crate::objects::OBJECT_SLOTS {
                        if o.type_frame[e] & 0x3ff == EGG && o.location[e] == at {
                            o.obj_status[e] &= !0x40;
                            o.qq[e] = (o.qq[e] & 0xff00) | 100;
                        }
                    }
                }
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
        for t in 0..4000 {
            if t >= 40 && self.at_prompt() {
                break;
            }
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

/// The port's Save of the start state against the shipped files, in both builds: the objlist
/// bytes the boot changes are the ones [`boot_change`] explains, the Save commits the region's
/// 2 × 2 surface blocks, and every block that differs holds the shipped records beside the
/// hatches' only ([`written_back_beside_hatches`]; in Original also the opening fight's
/// comparison, [`strip_fight`]). Prints what the oracle saw, as counts.
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
    let region = |blocks: &[Vec<u8>]| -> Vec<Vec<[u8; 8]>> {
        committed.iter().map(|&i| records(&blocks[i])).collect()
    };
    let hatches = Hatches::of(&s, &region(&old_blocks), &region(&new_blocks));
    for (name, _, _) in block_diffs(&old_blocks, &new_blocks) {
        let i = (0..BLOCK_COUNT).find(|&i| block_file(i) == name).quiet();
        let (mut ra, mut rb) = (records(&old_blocks[i]), records(&new_blocks[i]));
        assert!(committed.contains(&i), "{name}: a block of the region");
        let seen = written_back_beside_hatches(&name, &ra, &rb, &hatches);
        let seen = seen.unwrap_or_else(|e| panic!("{e}"));
        eprintln!("{name}: {seen}");
        if cfg!(feature = "wide") {
            // Wide: the window offers the eggs past the wide view too (spec 13 Decision 6), so
            // any block of the region may hold hatchlings' records, of more kinds than the start
            // egg's gear; the oracle above is the comparison.
            if i == fight {
                assert!(gear_records(&rb) > 0, "the hatched creatures' gear");
            }
            continue;
        }
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
    }
}

/// Prints how `got` differs from `want` (objlist tables, tail bytes, block records); returns
/// the number of differing block files.
///
/// Content-free: table names, byte and record indices only (no counts of the user's records).
#[cfg(not(feature = "wide"))]
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
#[cfg(not(feature = "wide"))]
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
#[cfg(not(feature = "wide"))]
fn defined(r: &[u8; 8]) -> [u8; 8] {
    let mut r = *r;
    if r[0] & 0x18 == 0x08 {
        r[3] = 0;
    }
    r
}

#[cfg(not(feature = "wide"))]
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
    (r[0] & 0x38 == 0x38 && temporary(usize::from(r[1]))) || egg_record(r)
}

/// An egg's record (type 0x14f, frame 0: bytes 4-5 big-endian).
fn egg_record(r: &[u8; 8]) -> bool {
    r[4..6] == [1, 0x4f]
}

/// What the saved state says about the hatches, for [`hatch_rule`]. The temporary actors come
/// from the state (a Save packs only map objects and what they hold; actors stay linked,
/// `objects/blocks.rs:94-` `collect`), the egg tiles from the block records (packing a map
/// object clears its slot's type, `objects/blocks.rs:85`).
struct Hatches {
    /// Temporary actors in use (slots 0xe0..0x100): slot, type, tile.
    actors: Vec<(u16, u16, Position)>,
    /// The tiles of the eggs that may have hatched ([`offered_eggs`]), in all four blocks of the
    /// region, not only the record's: the scatter (0x121e88) moves a hatchling up to 3 tiles,
    /// across a block's edge when the egg is near one (a block is 128 tiles a side,
    /// [`leader_block`]).
    eggs: Vec<Position>,
    /// A silver serpent's segment offsets from its head (dx at A4-0x6582, dy at A4-0x657e, the
    /// segment number & 3) and the frames a segment or the tail gets (A4-0x658a, A4-0x6586):
    /// `objects/commands/eggs.rs:53-58` `SEG_DX` .. `TAIL_FRAME`, read from the user's game.
    segment_at: Vec<(i16, i16)>,
    segment_frames: Vec<u8>,
}

impl Hatches {
    /// The hatches of saved state `s`, whose region's blocks are `shipped` and `saved`.
    fn of(s: &State, shipped: &[Vec<[u8; 8]>], saved: &[Vec<[u8; 8]>]) -> Hatches {
        let o = &s.objects;
        let actors = (FIRST_TEMP_ACTOR..ACTOR_SLOTS as Slot)
            .filter(|&m| o.type_frame[m as usize] != 0)
            .map(|m| (m as u16, o.kind(m), o.location[m as usize]))
            .collect();
        let eggs = offered_eggs(shipped, saved);
        let data = &s.objcmds.data;
        let sb = |off: i32, j: i32| i16::from(data.a4_byte(off + j) as i8);
        let segment_at = (0..4).map(|j| (sb(-0x6582, j), sb(-0x657e, j))).collect();
        let segment_frames = (0..4)
            .flat_map(|j| [data.a4_byte(-0x658a + j), data.a4_byte(-0x6586 + j)])
            .map(|f| f & 0x3f)
            .collect();
        Hatches {
            actors,
            eggs,
            segment_at,
            segment_frames,
        }
    }

    /// Whether a temporary actor of type `kind` stands within 1 tile of `at` on each axis, on its
    /// level.
    fn beside(&self, kind: u16, at: Position) -> bool {
        self.actors
            .iter()
            .any(|&(_, k, p)| k == kind && p.z == at.z && within(p, at, 1))
    }
}

/// The tiles of the region's eggs that may have hatched: saved map egg records with the bit 0x40
/// that 0x1319b2 sets on every egg it is offered (`objects/commands/eggs.rs:392`), each matching
/// a shipped egg record of the region with the bits 0x40 and 0x02 masked (one shipped egg each).
/// A saved egg no shipped one matches gives no tile, so a record cannot vouch for itself.
fn offered_eggs(shipped: &[Vec<[u8; 8]>], saved: &[Vec<[u8; 8]>]) -> Vec<Position> {
    let masked = |r: &[u8; 8]| [r[0] & !0x42, r[1], r[2], r[3], r[4], r[5], r[6], r[7]];
    let on_map = |r: &&[u8; 8]| egg_record(r) && r[0] & 0x18 == 0;
    let mut left: Vec<[u8; 8]> = shipped
        .iter()
        .flatten()
        .filter(on_map)
        .map(masked)
        .collect();
    let mut out = Vec::new();
    for r in saved.iter().flatten().filter(on_map) {
        if r[0] & 0x40 == 0 {
            continue;
        }
        if let Some(i) = left.iter().position(|x| *x == masked(r)) {
            left.swap_remove(i);
            out.push(Position::from_packed([r[1], r[2], r[3]]));
        }
    }
    out
}

/// Whether `a` and `b` are at most `d` tiles apart on each axis.
fn within(a: Position, b: Position, d: i32) -> bool {
    (i32::from(a.x) - i32::from(b.x)).abs() <= d && (i32::from(a.y) - i32::from(b.y)).abs() <= d
}

/// The rule [`hatch_rule`] passed a record by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    /// Carried or readied by a temporary actor.
    Carried,
    /// Inside a hatchling's container.
    Contained,
    /// A temporary object on the map near an egg.
    Placed,
    /// A body part of a monster of more than one tile.
    Part,
    /// A silver serpent's body segment.
    Segment,
}

/// The rule by which saved record `k` of `saved` is a hatch's (0x1319b2), or `None`. The fields
/// as 0x10d376 packs them (`objects/blocks.rs:58-89` `pack_record`): bytes 1-2 little-endian are
/// the owner of an inventory record and the record index of a container content's container
/// (`:75-82`), bytes 1-3 a map record's tile, bytes 4-5 big-endian the type (low 10 bits) and
/// frame, byte 7 the quantity (`:71-74`).
///
/// - Carried (0x10) or readied (0x18): the temporary bit 0x20, and the owner is a temporary actor
///   in use. 0x13121a gives a hatchling its gear and treasure inside it
///   (`objects/commands/eggs.rs:198-` `equip_monster`, `:270-` `treasure`) with 0x10be02, which
///   sets the bit (`objects/moves.rs:283-` `create_in`), and the packer turns a content of an
///   actor into inventory (`objects/blocks.rs:60-69`).
/// - Contained (0x08): the temporary bit, and the container is an earlier record of the block (the
///   packer searches back from the content, `objects/blocks.rs:77-80`) that is a hatch's: the
///   chest 0x13121a gives and fills (`objects/commands/eggs.rs:273-301`), carried, or on the map
///   (Placed) when its hatchling found no tile: 0x1319b2 removes a hatchling after the first that
///   the scatter could not place, on the egg's tile (`objects/commands/eggs.rs:457-462`), the
///   removal moves its contents onto that tile (`objects/moves.rs:140-151`), and a move takes an
///   object's own contents along (`objects/moves.rs:86-87`), so the chest is a temporary map
///   record and its contents stay contained.
/// - On the map with the temporary bit: within 3 tiles of an egg on each axis, on its level.
///   0x1319b2 creates a template below 0x156 on the egg's tile (`objects/commands/eggs.rs:436-438`;
///   0x10bcee sets the bit, `objects/moves.rs:268`), 0x121e88 scatters every hatchling after the
///   first (`objects/commands/eggs.rs:455-457`) by `a + b - 5`, a and b in 1..=4
///   (`combat/mode.rs:110-118`), and a hatchling that finds no tile is removed on the egg's tile
///   (`objects/commands/eggs.rs:462`), its contents left there (`objects/moves.rs:118`).
/// - On the map without the bit: a body part 0x131646 makes and clears the bit of
///   (`objects/commands/eggs.rs:120-189` `spawn_monster`), within 1 tile on each axis of a
///   temporary actor it belongs to (0x10b9ca moves a part one tile from the monster's new tile,
///   `objects/parts.rs:188-192`): a type from 0x1aa with the tail frame 2 or 14, of a monster of
///   its own type (`objects/commands/eggs.rs:156-168`); a dragon part (0x19b) of frame 8, 16, 24
///   or 32, of a dragon (`:169-177`); a part 0x1a9 of frame 4 × d (d 0..8), of a monster 0x176
///   (`:178-184`).
/// - On the map without the bit, type 0x19d: a silver serpent's body segment, whose quantity byte
///   names its head (`objects/commands/eggs.rs:551`), a temporary serpent, at one of the segment
///   offsets from the head (`:552-555`) with a segment or tail frame (`:548`, `:561-562`, `:568`;
///   [`Hatches`]). Exact offsets assume the start save comes before any turn moves a serpent;
///   no real data exercises this rule (no segment in either build's start save), only the
///   synthetic test.
///
/// A hydra's heads are actors (`objects/commands/eggs.rs:578-` `hydra_heads` through 0x131646,
/// `:585`), so no block record.
fn hatch_rule(saved: &[[u8; 8]], k: usize, h: &Hatches) -> Option<Rule> {
    /// Monster types with parts (0x131646).
    const TAILED: u16 = 0x1aa;
    const DRAGON: u16 = 0x19b;
    const EIGHT: u16 = 0x176;
    const EIGHT_PART: u16 = 0x1a9;
    const SERPENT: u16 = 0x19d;
    let r = saved.get(k)?;
    let temp = r[0] & crate::objects::TEMPORARY != 0;
    let word = u16::from_le_bytes([r[1], r[2]]);
    let kind = u16::from_be_bytes([r[4], r[5]]) & 0x3ff;
    let frame = r[4] >> 2;
    let at = Position::from_packed([r[1], r[2], r[3]]);
    match r[0] & 0x18 {
        0x10 | 0x18 => (temp && h.actors.iter().any(|a| a.0 == word)).then_some(Rule::Carried),
        0x08 => {
            let parent = usize::from(word);
            let held = parent < k
                && matches!(
                    hatch_rule(saved, parent, h),
                    Some(Rule::Carried | Rule::Contained | Rule::Placed)
                );
            (temp && held).then_some(Rule::Contained)
        }
        _ if temp => h
            .eggs
            .iter()
            .any(|&e| e.z == at.z && within(e, at, 3))
            .then_some(Rule::Placed),
        _ => {
            let part = (kind >= TAILED && matches!(frame, 2 | 14) && h.beside(kind, at))
                || (kind == DRAGON && matches!(frame, 8 | 16 | 24 | 32) && h.beside(DRAGON, at))
                || (kind == EIGHT_PART && frame % 4 == 0 && frame < 32 && h.beside(EIGHT, at));
            if part {
                return Some(Rule::Part);
            }
            let head = h
                .actors
                .iter()
                .find(|a| a.0 == u16::from(r[7]) && a.1 == SERPENT);
            let segment = kind == SERPENT
                && h.segment_frames.contains(&frame)
                && head.is_some_and(|&(_, _, p)| {
                    p.z == at.z
                        && h.segment_at.iter().any(|&(dx, dy)| {
                            i32::from(p.x) + i32::from(dx) == i32::from(at.x)
                                && i32::from(p.y) + i32::from(dy) == i32::from(at.y)
                        })
                });
            segment.then_some(Rule::Segment)
        }
    }
}

/// What [`written_back_beside_hatches`] saw in a block: the saved records no shipped one matches
/// and the rule each passed by, the eggs compared, and whether the records were the shipped ones
/// exactly (no other record).
#[derive(Debug, Default)]
struct Seen {
    new: usize,
    by_rule: [usize; 5],
    eggs: usize,
    exact: bool,
}

impl std::fmt::Display for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [c, n, p, b, s] = self.by_rule;
        write!(
            f,
            "records not shipped {} (carried {c}, contained {n}, placed {p}, parts {b}, \
             segments {s}), eggs compared {}, exactly the shipped records {}",
            self.new, self.eggs, self.exact
        )
    }
}

/// Block `name`'s saved records against the shipped ones. An egg's status bits 0x40 and 0x02
/// are masked on both sides (0x1319b2 sets them on every offer, `objects/commands/eggs.rs:392`);
/// nothing else of an egg may change. Matched with container indices cleared (they
/// shift after new records): every shipped record is written back, and every other saved record
/// is a hatch's ([`hatch_rule`]); an egg record no shipped egg matches is refused outright. With
/// no other record the saved records are the shipped ones exactly, sorted, container indices
/// included. Errors name the block and a record index only.
fn written_back_beside_hatches(
    name: &str,
    shipped: &[[u8; 8]],
    saved: &[[u8; 8]],
    h: &Hatches,
) -> Result<Seen, String> {
    let egg_masked = |r: &[u8; 8]| {
        let mut r = *r;
        if egg_record(&r) {
            r[0] &= !0x42;
        }
        r
    };
    let plain = |r: &[u8; 8]| {
        let mut r = egg_masked(r);
        if r[0] & 0x18 == 0x08 {
            r[1] = 0;
            r[2] = 0;
        }
        r
    };
    let mut left: Vec<[u8; 8]> = shipped.iter().map(plain).collect();
    let mut new = Vec::new();
    for (k, r) in saved.iter().enumerate() {
        let r = plain(r);
        match left.iter().position(|x| *x == r) {
            Some(i) => {
                left.swap_remove(i);
            }
            None => new.push(k),
        }
    }
    if !left.is_empty() {
        return Err(format!("{name}: a shipped record not written back"));
    }
    let mut seen = Seen {
        new: new.len(),
        eggs: saved.iter().filter(|r| egg_record(r)).count(),
        ..Seen::default()
    };
    for &k in &new {
        if egg_record(&saved[k]) {
            return Err(format!(
                "{name}: saved record #{k} is an egg no shipped one matches"
            ));
        }
        let rule =
            hatch_rule(saved, k, h).ok_or(format!("{name}: saved record #{k} is no hatch's"))?;
        seen.by_rule[rule as usize] += 1;
    }
    if new.is_empty() {
        let sorted = |v: &[[u8; 8]]| {
            let mut v: Vec<[u8; 8]> = v.iter().map(egg_masked).collect();
            v.sort();
            v
        };
        if sorted(shipped) != sorted(saved) {
            return Err(format!("{name}: not the shipped records exactly"));
        }
        seen.exact = true;
    }
    Ok(seen)
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

// Original only: the original's run; Wide's window hatches more eggs (spec 13 Decision 6).
#[cfg(not(feature = "wide"))]
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
// Original only: the original's run; Wide's window hatches more eggs (spec 13 Decision 6).
#[cfg(not(feature = "wide"))]
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
    let mut last: String = t
        .transcript_line
        .iter()
        .map(|&b| crate::ui::wide::latin_char(b))
        .collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| crate::ui::wide::latin_char(b)));
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

    /// `T`, the cross-hair onto the party's first member after the leader, Return; then answers
    /// the conversation: "bye" at a line input, else space / N / Y until the prompt is back.
    /// Returns the actor talked to. The target is a party member, not the nearest actor (which
    /// hung on where every actor round the party walked: any change to a path picked another,
    /// one that may not ask for a line). The boot starts in combat (the shipped combat byte,
    /// 0x121c64), so the member stands where the combat AI put it, not in a follow formation;
    /// the reach assert reports when it is past the cross-hair's box ([`crate::geom::RULE_HALF`]).
    fn talk(&mut self) -> u16 {
        let (target, dx, dy) = {
            let s = self.e.state();
            let c = s.leader_position().quiet();
            let leader = s.actors.party.leader().quiet().0;
            let target = s.actors.party.active().map(|a| a.0).find(|&a| a != leader);
            let target = target.expect("a member besides the leader");
            let p = s.objects.location[usize::from(target)];
            let (dx, dy) = (
                i32::from(p.x) - i32::from(c.x),
                i32::from(p.y) - i32::from(c.y),
            );
            let reach = i32::from(crate::geom::RULE_HALF);
            assert!(
                p.z == c.z && dx.abs() <= reach && dy.abs() <= reach,
                "the member is in the cross-hair's reach"
            );
            (target, dx, dy)
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

/// The start egg after the game's own boot (seed 0, A4-0x7afb clear: 0x1321dc offers it and
/// 0x1319b2 hatches it): its tile and the types of its templates. It is the hatched egg (status
/// 0x40) nearest the leader on the leader's level.
fn start_egg(s: &State) -> (Position, Vec<u16>) {
    let mut s = s.clone();
    let c = s.leader_position().quiet();
    let o = &s.objects;
    let egg = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&e| o.kind(e) == EGG && o.place(e) == 0 && o.z(e) == c.z && hatched(o, e))
        .min_by_key(|&e| {
            (i32::from(o.x(e)) - i32::from(c.x)).abs() + (i32::from(o.y(e)) - i32::from(c.y)).abs()
        })
        .expect("a hatched egg");
    let at = o.location[egg as usize];
    let mut kinds = Vec::new();
    let mut t = crate::objects::first_child(&mut s.objects, egg);
    while t >= 0 {
        kinds.push(s.objects.kind(t));
        t = crate::objects::next_child(&mut s.objects);
    }
    (at, kinds)
}

/// The egg type 0x1321dc and 0x1319b2 compare with.
const EGG: u16 = 0x14f;

/// Whether object `e` has the hatched bit 0x40 (0x1319b2 sets it, with 0x02, on every offer).
fn hatched(o: &crate::objects::Objects, e: Slot) -> bool {
    o.status(e) & 0x40 != 0
}

/// What [`walk_until_the_window_moves`] found.
struct Walked {
    /// Steps walked.
    steps: usize,
    /// Whether the window origin moved (the walk stops then, or after 40 steps).
    moved: bool,
    /// Ticks watched.
    ticks: usize,
    /// (step, slot) of every newcomer [`newcomers`] found.
    newcomers: Vec<(usize, Slot)>,
    /// The steps after which the start egg lacked its hatched bit.
    unmarked: Vec<usize>,
}

impl Run {
    /// [`Run::press`] with `watch` run on the state after every tick.
    fn press_watched(&mut self, k: Key, watch: &mut dyn FnMut(&State)) {
        self.e.tick(&[InputEvent::Key(k)], 16);
        watch(&self.e.state());
        for t in 0..4000 {
            if t >= 40 && self.at_prompt() {
                break;
            }
            self.e.tick(&[], 16);
            watch(&self.e.state());
        }
    }
}

/// The kind (type, the low 10 bits of the type + frame word) of every temporary-actor slot
/// (0xe0..0x100) of `s`, 0 for an empty slot.
fn slot_kinds(s: &State) -> Vec<u16> {
    (FIRST_TEMP_ACTOR as usize..ACTOR_SLOTS)
        .map(|m| s.objects.type_frame[m] & 0x3ff)
        .collect()
}

/// The newcomers between two looks at the temporary-actor slots: the indices whose kind is now
/// one of `kinds` (the start egg's) and differs from the kind `prior` held there (0: empty).
/// Kinds, not type + frame words: idle animation turns an actor's frame as display time passes
/// (0x10202e) and makes no newcomer. Residual: a slot freed and filled again with the same kind
/// between two looks shows the same kind, so that newcomer is not seen by type.
fn newcomers(prior: &[u16], now: &[u16], kinds: &[u16]) -> Vec<usize> {
    prior
        .iter()
        .zip(now)
        .enumerate()
        .filter(|&(_, (p, n))| n != p && kinds.contains(n))
        .map(|(i, _)| i)
        .collect()
}

/// Walks one step at a time (as [`Run::walk`]: south, else east, west, north) until the window
/// origin moves, at most 40 steps, looking at the temporary-actor slots after every tick for
/// [`newcomers`] of the start egg's `kinds`, from the kinds after the boot on. After each step it
/// looks at the start egg on tile `at`.
fn walk_until_the_window_moves(run: &mut Run, at: Position, kinds: &[u16]) -> Walked {
    let s = run.state();
    let mut prior = slot_kinds(&s);
    let origin = s.map.window_origin;
    drop(s);
    let mut walked = Walked {
        steps: 0,
        moved: false,
        ticks: 0,
        newcomers: Vec::new(),
        unmarked: Vec::new(),
    };
    let dirs = [Key::Down, Key::Right, Key::Left, Key::Up];
    while !walked.moved && walked.steps < 40 {
        walked.steps += 1;
        let step = walked.steps;
        let mut watch = |s: &State| {
            walked.ticks += 1;
            let now = slot_kinds(s);
            for i in newcomers(&prior, &now, kinds) {
                walked.newcomers.push((step, FIRST_TEMP_ACTOR + i as Slot));
            }
            prior = now;
        };
        let start = run.leader_at();
        let mut d = 0;
        for _ in 0..12 {
            run.press_watched(dirs[d], &mut watch);
            assert!(run.at_prompt(), "a step ends at the prompt");
            if run.leader_at() != start {
                break;
            }
            d = (d + 1) % dirs.len();
        }
        assert!(run.leader_at() != start, "the party cannot walk");
        let s = run.e.state();
        let o = &s.objects;
        let egg = (0x100..crate::objects::OBJECT_SLOTS as Slot)
            .find(|&e| o.kind(e) == EGG && o.place(e) == 0 && o.location[e as usize] == at)
            .expect("the start egg on its tile");
        if !hatched(o, egg) {
            walked.unmarked.push(step);
        }
        walked.moved = s.map.window_origin != origin;
    }
    walked
}

/// The newcomer detector on synthetic kinds: a slot that turns from start-egg kind A to start-egg
/// kind B, an empty slot that takes a start-egg kind, and a slot of another kind that turns into
/// one are newcomers; a start-egg kind kept, one that leaves, and another kind arriving are not.
#[test]
fn a_slot_that_changes_into_a_start_egg_kind_is_a_newcomer() {
    let (a, b, other) = (0x170, 0x171, 0x180);
    let kinds = [a, b];
    let prior = [a, 0, other, a, a, 0, other];
    let now = [b, a, a, a, 0, other, other];
    assert_eq!(newcomers(&prior, &now, &kinds), [0, 1, 2]);
    // The kinds come from the type + frame words: another frame of the same type is the same kind.
    let mut s = State::empty();
    s.objects.type_frame[FIRST_TEMP_ACTOR as usize] = a | 3 << 10;
    let before = slot_kinds(&s);
    s.objects.type_frame[FIRST_TEMP_ACTOR as usize] = a | 5 << 10;
    assert!(
        newcomers(&before, &slot_kinds(&s), &kinds).is_empty(),
        "an idle frame"
    );
    s.objects.type_frame[FIRST_TEMP_ACTOR as usize] = b | 5 << 10;
    assert_eq!(newcomers(&before, &slot_kinds(&s), &kinds), [0], "A to B");
}

/// The game's own boot hatches the start egg once: after the boot the egg has the bit 0x20 that
/// makes 0x1321dc offer it at every window rebuild, near the leader or not; walking on (in
/// combat, as the boot starts) until the window origin moves, which rebuilds the window and
/// offers it again, no temporary-actor slot newly holds one of its types on any tick (no second
/// wave) and the egg keeps its hatched bit, in both builds (the origin moves at other cells in
/// Wide, `geom::LEAD`). Prints counts the test made only.
#[test]
fn the_start_egg_hatches_once_when_the_window_moves() {
    let Some(files) = zip_files() else { return };
    let mut run = Run::boot(files);
    let s = run.state();
    let (at, kinds) = start_egg(&s);
    let o = &s.objects;
    let egg = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .find(|&e| o.kind(e) == EGG && o.place(e) == 0 && o.location[e as usize] == at)
        .quiet();
    assert!(
        o.status(egg) & 0x20 != 0,
        "the start egg is offered at every rebuild (bit 0x20)"
    );
    let hatchlings = (FIRST_TEMP_ACTOR..ACTOR_SLOTS as Slot)
        .any(|m| o.type_frame[m as usize] != 0 && kinds.contains(&o.kind(m)));
    assert!(hatchlings, "the boot hatched the start egg");
    drop(s);
    let w = walk_until_the_window_moves(&mut run, at, &kinds);
    eprintln!(
        "the window origin moved after {} steps ({} ticks watched): {} newcomers, {} steps \
         without the hatched bit",
        w.steps,
        w.ticks,
        w.newcomers.len(),
        w.unmarked.len()
    );
    assert!(w.moved, "the window origin moved within 40 steps");
    assert!(w.newcomers.is_empty(), "no second wave of the start egg");
    assert!(w.unmarked.is_empty(), "the start egg keeps its hatched bit");
}

/// The newcomer watch sees a second wave: the start egg made unhatched and full after the boot
/// ([`Cmd::Rearm`]) hatches again when the walk moves the window, and its new hatchlings are
/// newcomers. Prints counts the test made only.
#[test]
fn a_forced_second_hatch_of_the_start_egg_is_seen() {
    let Some(files) = zip_files() else { return };
    let mut run = Run::boot(files);
    let s = run.state();
    let (at, kinds) = start_egg(&s);
    let free =
        (FIRST_TEMP_ACTOR..ACTOR_SLOTS as Slot).any(|m| s.objects.type_frame[m as usize] == 0);
    assert!(free, "a free temporary-actor slot for a second wave");
    drop(s);
    run.command(Cmd::Rearm(at));
    let w = walk_until_the_window_moves(&mut run, at, &kinds);
    eprintln!(
        "forced: the window origin moved after {} steps; newcomers seen {}, the first at step \
         {:?}",
        w.steps,
        !w.newcomers.is_empty(),
        w.newcomers.first().map(|n| n.0)
    );
    assert!(w.moved, "the window origin moved within 40 steps");
    assert!(!w.newcomers.is_empty(), "the second wave is seen");
    assert!(
        w.newcomers.iter().all(|&(step, _)| step == w.steps),
        "the second wave comes at the step the window moved"
    );
}

/// A synthetic map record: `status`, tile (x, y, z), type `kind` with `frame`, quantity `qty`.
fn map_record(status: u8, (x, y, z): (u16, u16, u8), kind: u16, frame: u16, qty: u8) -> [u8; 8] {
    let [b1, b2, b3] = Position { x, y, z }.to_packed();
    let [t0, t1] = (kind | frame << 10).to_be_bytes();
    [status, b1, b2, b3, t0, t1, 0, qty]
}

/// A synthetic held record: `status` (its location bits 0x08, 0x10 or 0x18), the owner or the
/// container's record index `word`, type `kind`.
fn held_record(status: u8, word: u16, kind: u16) -> [u8; 8] {
    let [lo, hi] = word.to_le_bytes();
    let [t0, t1] = kind.to_be_bytes();
    [status, lo, hi, 0, t0, t1, 0, 1]
}

/// The errors of [`written_back_beside_hatches`], by the words each carries.
const NOT_BACK: &str = "not written back";
const NO_HATCH: &str = "is no hatch's";
const NEW_EGG: &str = "is an egg no shipped one matches";
const NOT_EXACT: &str = "not the shipped records exactly";

/// The save oracle on synthetic records (no game data): every rule of [`hatch_rule`] takes its
/// own kind of record, and each record or change no hatch makes is refused with its own error,
/// among them a map record of an actor type far from any temporary actor, which the round-1 rule
/// (temporary, or a type from 0x156) took.
#[test]
fn the_save_oracle_takes_only_what_a_hatch_makes() {
    let p = |x, y| Position { x, y, z: 0 };
    let h = Hatches {
        actors: vec![
            (0xe0, 0x1b0, p(100, 100)),
            (0xe1, 0x19b, p(200, 200)),
            (0xe2, 0x176, p(300, 300)),
            (0xe3, 0x19d, p(400, 400)),
        ],
        eggs: vec![p(50, 50)],
        segment_at: vec![(0, 1), (1, 0), (0, -1), (-1, 0)],
        segment_frames: vec![1, 2],
    };
    let egg = map_record(0x20, (50, 50, 0), EGG, 0, 1);
    let shipped = vec![egg, map_record(0x01, (10, 10, 0), 0x30, 0, 1)];
    let check = |saved: &[[u8; 8]]| written_back_beside_hatches("block", &shipped, saved, &h);
    let with = |extra: &[[u8; 8]]| {
        let mut v = shipped.clone();
        v.extend_from_slice(extra);
        v
    };
    // The egg's status bits 0x40 and 0x02 alone changed: the shipped records exactly.
    let mut marked = shipped.clone();
    marked[0][0] |= 0x42;
    let seen = check(&marked).quiet();
    assert!(
        seen.exact && seen.new == 0 && seen.eggs == 1,
        "a marked egg is the shipped one"
    );
    // One record of every rule (the chest, record 3, holds the content, record 4).
    let all = with(&[
        held_record(0x31, 0xe0, 0x58),
        held_record(0x31, 0xe0, 0x62),
        held_record(0x29, 3, 0x58),
        map_record(0x21, (52, 47, 0), 0x40, 0, 1),
        map_record(0x01, (101, 100, 0), 0x1b0, 2, 1),
        map_record(0x01, (200, 199, 0), 0x19b, 8, 1),
        map_record(0x01, (301, 299, 0), 0x1a9, 12, 1),
        map_record(0x01, (400, 401, 0), 0x19d, 1, 0xe3),
    ]);
    let seen = check(&all).quiet();
    assert_eq!(
        seen.by_rule,
        [2, 1, 1, 3, 1],
        "carried, contained, placed, parts, segments"
    );
    assert!(!seen.exact && seen.new == 8);
    // A hatchling the scatter could not place, removed on the egg's tile: its chest is a
    // temporary map record there (record 2), the chest's content still contained (record 3).
    let dropped = with(&[
        map_record(0x21, (50, 50, 0), 0x62, 1, 1),
        held_record(0x29, 2, 0x58),
    ]);
    let seen = check(&dropped).quiet();
    assert_eq!(
        seen.by_rule,
        [0, 1, 1, 0, 0],
        "a dropped chest and its content"
    );
    // The round-1 rule took any map record that is temporary or of an actor type.
    let old = |r: &[u8; 8]| r[0] & 0x20 != 0 || u16::from_be_bytes([r[4], r[5]]) & 0x3ff >= 0x156;
    let far_part = map_record(0x01, (600, 600, 0), 0x1b0, 2, 1);
    assert!(old(&far_part));
    let up = |(x, y): (u16, u16)| (x, y, 1u8);
    let refused: Vec<(&str, &str, Vec<[u8; 8]>)> = vec![
        (
            "an actor-type map record far from any temporary actor",
            NO_HATCH,
            with(&[far_part]),
        ),
        (
            "a tail beside a monster of another type",
            NO_HATCH,
            with(&[map_record(0x01, (201, 200, 0), 0x1b0, 2, 1)]),
        ),
        (
            "a tail of another frame",
            NO_HATCH,
            with(&[map_record(0x01, (101, 100, 0), 0x1b0, 5, 1)]),
        ),
        (
            "a dragon part of another frame",
            NO_HATCH,
            with(&[map_record(0x01, (200, 199, 0), 0x19b, 12, 1)]),
        ),
        (
            "a part 0x1a9 of a frame not a multiple of 4",
            NO_HATCH,
            with(&[map_record(0x01, (301, 299, 0), 0x1a9, 13, 1)]),
        ),
        (
            "a part 0x1a9 of frame 32",
            NO_HATCH,
            with(&[map_record(0x01, (301, 299, 0), 0x1a9, 32, 1)]),
        ),
        (
            "a part on another level",
            NO_HATCH,
            with(&[map_record(0x01, up((101, 100)), 0x1b0, 2, 1)]),
        ),
        (
            "a temporary map record far from the eggs",
            NO_HATCH,
            with(&[map_record(0x21, (54, 50, 0), 0x40, 0, 1)]),
        ),
        (
            "a temporary map record near an egg on another level",
            NO_HATCH,
            with(&[map_record(0x21, up((51, 50)), 0x40, 0, 1)]),
        ),
        (
            "one carried by an actor not in use",
            NO_HATCH,
            with(&[held_record(0x31, 0xe9, 0x58)]),
        ),
        (
            "one carried without the temporary bit",
            NO_HATCH,
            with(&[held_record(0x11, 0xe0, 0x58)]),
        ),
        (
            "a content of a record no hatch made",
            NO_HATCH,
            with(&[held_record(0x29, 1, 0x58)]),
        ),
        (
            "a content whose container index is its own",
            NO_HATCH,
            with(&[held_record(0x29, 2, 0x58)]),
        ),
        (
            "a content whose container comes after it",
            NO_HATCH,
            with(&[held_record(0x29, 3, 0x58), held_record(0x31, 0xe0, 0x62)]),
        ),
        (
            "a segment whose head is no serpent",
            NO_HATCH,
            with(&[map_record(0x01, (101, 101, 0), 0x19d, 1, 0xe0)]),
        ),
        (
            "a segment off the offsets",
            NO_HATCH,
            with(&[map_record(0x01, (402, 400, 0), 0x19d, 1, 0xe3)]),
        ),
        (
            "an added egg",
            NEW_EGG,
            with(&[map_record(0x60, (51, 50, 0), EGG, 0, 1)]),
        ),
        (
            "a moved egg",
            NOT_BACK,
            vec![map_record(0x20, (51, 50, 0), EGG, 0, 1), shipped[1]],
        ),
        ("an egg changed past bits 0x40 and 0x02", NOT_BACK, {
            let mut v = shipped.clone();
            v[0][0] |= 0x01;
            v
        }),
        ("a shipped record left out", NOT_BACK, vec![egg]),
        ("an egg left out", NOT_BACK, vec![shipped[1]]),
    ];
    for (what, error, saved) in &refused {
        let got = check(saved).err().unwrap_or_default();
        assert!(got.contains(error), "refused: {what}: {error}");
    }
    // Nothing new, but a container index changed: not the shipped records exactly.
    let held = vec![
        egg,
        map_record(0x01, (10, 10, 0), 0x62, 0, 1),
        held_record(0x09, 1, 0x58),
    ];
    let moved = vec![held[0], held[1], held_record(0x09, 0, 0x58)];
    let check_held = |saved: &[[u8; 8]]| written_back_beside_hatches("block", &held, saved, &h);
    assert!(check_held(&held).is_ok_and(|s| s.exact));
    let got = check_held(&moved).err().unwrap_or_default();
    assert!(
        got.contains(NOT_EXACT),
        "refused: a container index changed, nothing new: {NOT_EXACT}"
    );
}

/// The eggs [`Hatches::of`] takes its tiles from (no game data): only a saved egg with the hatch's
/// bit 0x40 that matches a shipped egg. A temporary egg appended where no shipped egg is gives no
/// tile, so neither it nor an object beside it passes as a hatch's; a shipped egg without the
/// bit gives none either.
#[test]
fn an_egg_the_shipped_blocks_lack_vouches_for_nothing() {
    let p = |x, y| Position { x, y, z: 0 };
    let egg = map_record(0x20, (50, 50, 0), EGG, 0, 1);
    let plain = map_record(0x01, (10, 10, 0), 0x30, 0, 1);
    let mut marked = egg;
    marked[0] |= 0x42;
    let added = map_record(0x60, (500, 500, 0), EGG, 0, 1);
    let near_added = map_record(0x21, (500, 501, 0), 0x40, 0, 1);
    let near_shipped = map_record(0x21, (50, 51, 0), 0x40, 0, 1);
    let shipped = vec![vec![egg, plain]];
    let run = |saved: Vec<[u8; 8]>| {
        let blocks = vec![saved.clone()];
        let h = Hatches::of(&State::empty(), &shipped, &blocks);
        (
            h.eggs.clone(),
            written_back_beside_hatches("block", &shipped[0], &saved, &h),
        )
    };
    let (eggs, got) = run(vec![marked, plain, added, near_added]);
    assert_eq!(eggs, [p(50, 50)], "only the shipped egg gives a tile");
    assert!(
        got.err().unwrap_or_default().contains(NEW_EGG),
        "the added egg"
    );
    let (_, got) = run(vec![marked, plain, near_added]);
    assert!(
        got.err().unwrap_or_default().contains(NO_HATCH),
        "beside no offered egg"
    );
    let (_, got) = run(vec![marked, plain, near_shipped]);
    assert!(
        got.is_ok_and(|s| s.by_rule[2] == 1),
        "beside the offered egg"
    );
    let (eggs, got) = run(vec![egg, plain, near_shipped]);
    assert!(
        eggs.is_empty(),
        "an egg without the bit 0x40 was not offered"
    );
    assert!(
        got.err().unwrap_or_default().contains(NO_HATCH),
        "beside an egg never offered"
    );
}

/// The spells [`give_spellbook`] puts in the book: each of the first three circles' spells the
/// magic tables name (spell numbers 0..0x30, circle × 16 + index).
fn book_spells(s: &State) -> Vec<u8> {
    (0u8..0x30)
        .filter(|&n| !s.magic.tables.name(usize::from(n)).is_empty())
        .collect()
}

/// The hero (the party's first member, the view's actor at the start) given a readied spellbook
/// holding [`book_spells`] and 10 of each reagent, through the objects API as
/// `magic::tests_real`'s `give_book` does.
fn give_spellbook(s: &mut State) {
    let hero = Slot::from(s.actors.party.members[0]);
    let spells = book_spells(s);
    let book =
        crate::objects::create_in(s, crate::magic::SPELLBOOK, crate::objects::READIED, 0, hero);
    for n in spells {
        crate::objects::create_in(
            s,
            crate::magic::SPELL,
            crate::objects::IN_CONTAINER,
            u16::from(n) << 8,
            book,
        );
    }
    for kind in s.magic.tables.reagents {
        crate::objects::add_to_owner(s, hero, kind, 10);
    }
}

/// The spell numbers inside each spellbook the hero has readied, each book's in ascending order
/// (a new object is linked right after its owner, so the list holds a book's spells in no fixed
/// order).
fn readied_books(s: &State) -> Vec<Vec<u8>> {
    let mut s = s.clone();
    let hero = Slot::from(s.actors.party.members[0]);
    let o = &mut s.objects;
    // The child walk passes over readied objects while this is set.
    o.skip_readied = 0;
    let mut books = Vec::new();
    let mut c = crate::objects::first_child(o, hero);
    while c >= 0 {
        if o.place(c) == crate::objects::READIED && o.kind(c) == crate::magic::SPELLBOOK {
            books.push(c);
        }
        c = crate::objects::next_child(o);
    }
    books
        .into_iter()
        .map(|book| {
            let mut spells = Vec::new();
            let mut c = crate::objects::first_child(o, book);
            while c >= 0 {
                if o.kind(c) == crate::magic::SPELL {
                    spells.push((o.qq_of(c) >> 8) as u8);
                }
                c = crate::objects::next_child(o);
            }
            spells.sort_unstable();
            spells
        })
        .collect()
}

/// Spec 08 (2026-10-25): a save for the owner's own look at the spellbook page in Korean. With
/// `U6_BOOK_SAVE=<dir>` (an absolute path outside the repository) and the zip present: the boot
/// of his files to the prompt, the hero given a readied spellbook holding the first circle's
/// spells and the next two circles' ([`give_spellbook`]), the engine's own Save, its files
/// handed to a store as the platform's flush hands them ([`SaveSync::flush`]) and written to
/// `<dir>/savegame/<name>` under the original's names: the folder the page takes as the save to
/// start from. A fresh engine booted from the files read back from there has that book readied
/// on the hero, holding those spells. Without the variable it does nothing. Prints the count of
/// files and pass/fail only.
#[test]
fn with_u6_book_save_a_save_with_a_readied_spellbook_is_written_there() {
    let Some(dir) = std::env::var_os("U6_BOOK_SAVE") else {
        return;
    };
    let Some(files) = zip_files() else { return };
    let dir = std::path::PathBuf::from(dir);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .quiet();
    // Outside the repository, symlinks resolved: checked before anything is written.
    let lexical = dir.is_absolute()
        && dir
            .components()
            .all(|c| !matches!(c, std::path::Component::ParentDir));
    let existing = dir.ancestors().find(|a| a.exists()).map(Path::canonicalize);
    let outside = |p: &Path| !p.starts_with(&repo);
    assert!(
        lexical && outside(&dir) && existing.is_some_and(|e| e.is_ok_and(|e| outside(&e))),
        "U6_BOOK_SAVE is an absolute path outside the repository"
    );
    let mut run = Run::boot(files.clone());
    let spells = book_spells(&run.state());
    assert!(!spells.is_empty(), "the first circles name spells");
    run.command(Cmd::Edit(give_spellbook));
    assert!(
        readied_books(&run.state()).contains(&spells),
        "the hero holds the readied book"
    );
    run.command(Cmd::Save);
    let mut store = MemStore::default();
    let written = SaveSync::default().flush(&run.e, &mut store).quiet();
    assert!(
        written > 0 && written == store.files.len(),
        "the Save hands its files to the store"
    );
    let folder = dir.join("savegame");
    std::fs::create_dir_all(&folder).quiet();
    assert!(
        outside(&folder.canonicalize().quiet()),
        "the folder made lies outside the repository"
    );
    let mut back = MemStore::default();
    for (name, bytes) in &store.files {
        let bare = name.strip_prefix("savegame/").quiet();
        assert!(
            !bare.is_empty() && !bare.contains(['/', '\\']),
            "a save file's name is one file of savegame/"
        );
        std::fs::write(folder.join(bare), bytes).quiet();
        back.files
            .insert(name.clone(), std::fs::read(folder.join(bare)).quiet());
    }
    eprintln!("book save: {written} files written to U6_BOOK_SAVE/savegame");
    // The page lays the folder over the game files; the boot reads it as the save to start from.
    let mut f = files;
    assert!(
        attach(&mut f, &back).quiet() == written,
        "every file laid over"
    );
    let fresh = Run::boot(f);
    assert!(
        readied_books(&fresh.state()) == [spells],
        "booted from the files written, the hero holds the readied book with its spells"
    );
    assert!(
        book_equipped(&fresh.state()),
        "the inventory view puts the book in an equipment slot"
    );
    eprintln!(
        "book save: booted back, the hero holds the readied spellbook with its spells, in an equipment slot: pass"
    );
}

/// Whether the inventory's count of the hero's objects (0x110f96 by way of the weights, as the
/// inventory view runs it) puts a readied spellbook of his in an equipment slot.
fn book_equipped(s: &State) -> bool {
    let mut s = s.clone();
    let hero = Slot::from(s.actors.party.members[0]);
    crate::inventory::carried_weight(&mut s, hero);
    (0x100..crate::objects::OBJECT_SLOTS as Slot).any(|o| {
        s.objects.kind(o) == crate::magic::SPELLBOOK
            && s.objects.place(o) == crate::objects::READIED
            && s.objects.parent(o) == hero
            && crate::inventory::equipped_index(&s, o) >= 0
    })
}

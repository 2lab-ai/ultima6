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

use super::objlist::{OBJLIST_LEN, TABLES, decode, encode};
use super::*;
use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::objects::BLOCK_COUNT;
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
const ORACLE: &str = "/Users/zhugehyuk/Games/ultima6/oracle/saveload";

fn zip_files() -> Option<Files> {
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
        files.insert(format!("savegame/{name}"), std::fs::read(e.path()).unwrap());
    }
    Some(MemStore { files })
}

/// What the test asks the game to do at its next poll.
enum Cmd {
    Save,
    Restore,
}

/// The real game future (`main_loop::run`) with commands run at the start of a poll, inside
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
                Cmd::Restore => restore(&mut s, &self.io).unwrap_or_else(|e| panic!("{e}")),
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
    /// Boots `files` as `Engine::new` does (seed 0) and runs to the first command prompt.
    fn boot(files: Files) -> Run {
        let mut state = State::new_game(&files).unwrap_or_else(|e| panic!("{e}"));
        state.rng.seed(0);
        let cmds = Rc::new(RefCell::new(VecDeque::new()));
        let c = Rc::clone(&cmds);
        let e = Engine::with_game(state, files, move |st, io| Inject {
            game: Box::pin(crate::main_loop::run(Rc::clone(&st), Rc::clone(&io))),
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
    (c.saveload.objlist.unwrap(), blocks)
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
        .map(|r| r.try_into().unwrap())
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
    let raw = files.get(OBJLIST_FILE).unwrap();
    let list = ObjList::parse(raw).unwrap();
    let mut s = State::empty();
    decode(&mut s, raw);
    let (o, a) = (&s.objects, &s.actors);
    for (i, r) in list.actors.iter().enumerate() {
        assert_eq!(o.obj_status[i], r.obj_flags, "{i}");
        assert_eq!(o.location[i], r.pos, "{i}");
        assert_eq!(o.type_frame[i] & 0x3ff, r.obj_n, "{i}");
        assert_eq!((o.type_frame[i] >> 10) as u8, r.frame, "{i}");
        assert_eq!(o.qq[i], r.unknown_0600, "{i}");
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
        assert_eq!(got, want, "actor {i}");
        assert_eq!(t.exp[i], r.exp);
        assert_eq!(t.base_type_frame[i] & 0x3ff, r.base_obj_n);
        assert_eq!(t.unknown_18f1[i], raw[0x18F1 + i]);
        assert_eq!(t.unknown_1af1[i], raw[0x1AF1 + i]);
    }
    assert_eq!(a.party.names, list.party.names);
    assert_eq!((a.party.members, a.party.size), (list.party.roster, list.party.size));
    let c = list.clock;
    assert_eq!(
        (a.calendar.minute, a.calendar.hour, a.calendar.day, a.calendar.month, a.calendar.year),
        (c.minute, c.hour, c.day, c.month, c.year)
    );
    let p = list.player;
    assert_eq!(
        (a.quest_flag, a.rest_counter, a.karma, a.wind),
        (p.quest_flag, p.rest_counter, p.karma, p.wind_direction)
    );
    assert_eq!(a.timers, list.timers);
    assert_eq!(a.objlist_tail, raw[0x1BF1..0x1C73]);
    // The view centre is not decoded (0x105350 takes it from the leader), so a state that
    // holds the file's centre words writes the file's first 0x1C73 bytes back.
    let t = &raw[0x1BF1 + 0x0c..0x1BF1 + 0x12];
    let w = |i: usize| u16::from_be_bytes([t[i], t[i + 1]]);
    s.map.centre = u6_formats::position::Position {
        x: w(0),
        y: w(2),
        z: w(4) as u8,
    };
    assert_eq!(encode(&s), raw[..OBJLIST_LEN]);
    eprintln!(
        "shipped objlist {} bytes; the game reads and writes {OBJLIST_LEN}",
        raw.len()
    );
}

/// The new-game files the port boots from: objlist and the 69 blocks.
fn shipped(files: &Files) -> (Vec<u8>, Vec<Vec<u8>>) {
    let objlist = files.get(OBJLIST_FILE).unwrap()[..OBJLIST_LEN].to_vec();
    let blocks = (0..BLOCK_COUNT)
        .map(|i| files.get(&block_file(i)).unwrap().to_vec())
        .collect();
    (objlist, blocks)
}

#[test]
fn a_save_of_the_start_state_writes_the_shipped_bytes_back() {
    let Some(files) = zip_files() else { return };
    let (old_list, old_blocks) = shipped(&files);
    let mut run = Run::boot(files);
    run.command(Cmd::Save);
    let s = run.state();
    let new_list = s.saveload.objlist.clone().unwrap();
    let new_blocks: Vec<Vec<u8>> = s.objects.blocks.iter().map(|(_, b)| b.to_bytes()).collect();
    for d in table_diffs(&old_list, &new_list) {
        eprintln!("objlist differs: {d:?}");
    }
    let committed: Vec<String> = (0..BLOCK_COUNT)
        .filter(|&i| s.saveload.block_gen[i] == 1)
        .map(block_file)
        .collect();
    eprintln!("committed: {committed:?}");
    for d in block_diffs(&old_blocks, &new_blocks) {
        eprintln!("block differs: {d:?}");
    }
    for i in 0..BLOCK_COUNT {
        let (a, b) = (records(&old_blocks[i]), records(&new_blocks[i]));
        let mut shown = 0;
        for (n, (x, y)) in a.iter().zip(&b).enumerate() {
            if x != y && shown < 12 {
                eprintln!("  {} #{n}: {:02x?} -> {:02x?}", block_file(i), x, y);
                shown += 1;
            }
        }
        if a != b {
            let mut sa = a.clone();
            let mut sb = b.clone();
            sa.sort();
            sb.sort();
            eprintln!("  {}: same records as a set: {}", block_file(i), sa == sb);
        }
    }
    let tail: Vec<String> = (0..0x82)
        .filter(|&i| old_list[0x1BF1 + i] != new_list[0x1BF1 + i])
        .map(|i| format!("{i:#04x}: {:#04x} -> {:#04x}", old_list[0x1BF1 + i], new_list[0x1BF1 + i]))
        .collect();
    eprintln!("tail: {tail:?}");
}

/// Prints how `got` differs from `want` (objlist tables, tail bytes, block records); returns
/// the number of differing block files.
fn report(what: &str, want: &(Vec<u8>, Vec<Vec<u8>>), got: &(Vec<u8>, Vec<Vec<u8>>)) -> usize {
    for d in table_diffs(&want.0, &got.0) {
        eprintln!("{what}: objlist {d:?}");
    }
    let tail: Vec<String> = (0..0x82)
        .filter(|&i| want.0.get(0x1BF1 + i) != got.0.get(0x1BF1 + i))
        .map(|i| format!("{i:#04x}: {:#04x} -> {:#04x}", want.0[0x1BF1 + i], got.0[0x1BF1 + i]))
        .collect();
    if !tail.is_empty() {
        eprintln!("{what}: tail {tail:?}");
    }
    let diffs = block_diffs(&want.1, &got.1);
    for (name, a, b) in &diffs {
        let i = (0..BLOCK_COUNT).find(|&i| &block_file(i) == name).unwrap();
        let (ra, rb) = (records(&want.1[i]), records(&got.1[i]));
        let (mut sa, mut sb) = (ra.clone(), rb.clone());
        sa.sort();
        sb.sort();
        eprintln!("{what}: {name} records {a} -> {b}, same set: {}", sa == sb);
        for (n, (x, y)) in ra.iter().zip(&rb).enumerate().filter(|(_, (x, y))| x != y).take(6) {
            eprintln!("    #{n}: {x:02x?} -> {y:02x?}");
        }
    }
    diffs.len()
}

/// The `savegame/` files of a store, objlist and the 69 blocks.
fn store_files(store: &MemStore) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut f = Files::default();
    f.overlay("s", store.files.clone());
    let objlist = f.get(OBJLIST_FILE).unwrap().to_vec();
    let blocks = (0..BLOCK_COUNT)
        .map(|i| f.get(&block_file(i)).unwrap().to_vec())
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

/// The opening fight the port does not run: the egg at the throne (0x1319b2, a stub) hatches
/// three gargoyles into the temporary actor slots 0xe0..0xe2, and combat mode starts
/// (0x121c64, a stub). Objlist bytes these change in the original's start save.
fn opening_fight(table: &str, index: usize) -> bool {
    let actor = |i: usize| match table {
        "0x135588 location" => i / 3,
        "0x137988 type + frame"
        | "A4+0x250c quality / quantity"
        | "A4+0x440c experience"
        | "0x139688 base type + frame" => i / 2,
        _ => i,
    };
    let a = actor(index);
    (0xe0..=0xe2).contains(&a)
        || (table == "0x139188 worktype" && (2..=4).contains(&a))
        || (table == "0x139488 worktype operand" && (1..=4).contains(&a))
        || (table == "0x137988 type + frame" && [3, 6].contains(&a))
        || (table == "A4-0x7b84 block" && index == 0x7a)
}

#[test]
fn the_ports_start_save_against_the_originals() {
    let Some(mut files) = zip_files() else { return };
    let (Some(boot), Some(saved)) = (oracle_save("start-boot"), oracle_save("start-saved")) else {
        return;
    };
    attach(&mut files, &boot).unwrap();
    let mut run = Run::boot(files);
    run.command(Cmd::Save);
    let s = run.state();
    let mut got_store = boot.clone();
    got_store.store(&changed_since(&s, 0)).unwrap();
    let want = store_files(&saved);
    let got = store_files(&got_store);
    assert_eq!(got.0.len(), want.0.len(), "objlist length");
    report("port vs original", &want, &got);
    for (off, len, name) in TABLES {
        for i in 0..len {
            if want.0[off + i] != got.0[off + i] {
                assert!(opening_fight(name, i), "{name} byte {i:#x} unexplained");
            }
        }
    }
    for i in 0..BLOCK_COUNT {
        let (w, g) = (defined_block(&want.1[i]), defined_block(&got.1[i]));
        if block_file(i) != "savegame/objblkCC" {
            assert_eq!(w, g, "{}", block_file(i));
            continue;
        }
        // CC: the gargoyles' readied gear (temporary, owned by 0xe0..0xe2) is extra in the
        // original, the egg's status differs, and the container indices after them shift.
        let fight = |r: &[u8; 8]| {
            (r[0] & 0x38 == 0x38 && (0xe0..=0xe2).contains(&r[1])) || r[4..6] == [1, 0x4f]
        };
        let strip = |v: Vec<[u8; 8]>| -> Vec<[u8; 8]> {
            let mut v: Vec<[u8; 8]> = v
                .into_iter()
                .filter(|r| !fight(r))
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
        };
        let extra = w.iter().filter(|r| fight(r)).count() - g.iter().filter(|r| fight(r)).count();
        eprintln!("objblkCC: {extra} records more in the original (the gargoyles' gear)");
        assert_eq!(extra, 13);
        assert_eq!(strip(w), strip(g), "CC apart from the fight");
    }
}

#[test]
fn the_originals_save_loads_and_saves_back_unchanged() {
    let Some(mut files) = zip_files() else { return };
    let Some(saved) = oracle_save("start-saved") else { return };
    assert_eq!(saved.files[OBJLIST_FILE].len(), OBJLIST_LEN);
    attach(&mut files, &saved).unwrap();
    let mut run = Run::boot(files);
    {
        let s = run.e.state();
        let types: Vec<u16> = (0xe0..0xe3).map(|a| s.objects.type_frame[a] & 0x3ff).collect();
        eprintln!("loaded: centre {:?}, temporary actors {types:x?}", s.map.centre);
        assert!(types.iter().all(|&t| t != 0), "the saved gargoyles are back");
        assert_eq!(s.saveload.generation, 0);
    }
    run.command(Cmd::Save);
    let s = run.state();
    let mut got_store = saved.clone();
    got_store.store(&changed_since(&s, 0)).unwrap();
    let want = store_files(&saved);
    let got = store_files(&got_store);
    report("port re-save vs original", &want, &got);
    let diff: Vec<(&str, usize)> = TABLES
        .iter()
        .flat_map(|&(off, len, name)| {
            (0..len)
                .filter(move |&i| want.0[off + i] != got.0[off + i])
                .map(move |i| (name, i))
        })
        .collect();
    eprintln!("objlist bytes that differ: {diff:x?}");
    for i in 0..BLOCK_COUNT {
        assert_eq!(
            defined_block(&want.1[i]),
            defined_block(&got.1[i]),
            "{}",
            block_file(i)
        );
    }
}

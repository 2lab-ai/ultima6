//! Save and restore on synthetic states.

use std::collections::BTreeMap;

use u6_formats::objblk::{ObjBlock, ObjBlockId, ObjRecord};
use u6_formats::position::Position;

use super::objlist::{OBJLIST_LEN, TABLES, TAIL, decode, encode};
use super::*;
use crate::files::Files;
use crate::state::State;

#[test]
fn the_tables_tile_the_file_in_order() {
    let mut at = 0;
    for (off, len, _) in TABLES {
        assert_eq!(off, at, "table at {off:#x}");
        at += len;
    }
    assert_eq!(at, OBJLIST_LEN);
}

#[test]
fn block_names_are_the_originals() {
    assert_eq!(block_file(0), "savegame/objblkAA");
    // Surface superchunk sx = 3, sy = 1.
    assert_eq!(block_file(8 + 3), "savegame/objblkDB");
    assert_eq!(block_file(63), "savegame/objblkHH");
    // Dungeon level z at 63 + z: OBJBLK{A+z-1}I.
    assert_eq!(block_file(64), "savegame/objblkAI");
    assert_eq!(block_file(68), "savegame/objblkEI");
    for (i, id) in ObjBlockId::all().enumerate() {
        let want = format!("savegame/{}", id.file_name());
        assert!(block_file(i).eq_ignore_ascii_case(&want), "{i}");
    }
}

/// A state with something distinct in every table.
pub(super) fn filled() -> State {
    let mut s = State::empty();
    for i in 0..256 {
        let b = i as u8;
        s.objects.obj_status[i] = b;
        s.objects.location[i] = Position {
            x: (i * 3) as u16 & 0x3ff,
            y: (i * 5) as u16 & 0x3ff,
            z: b & 0xf,
        };
        s.objects.type_frame[i] = 0x1234u16.wrapping_add(i as u16);
        s.objects.qq[i] = 0xabcdu16.wrapping_sub(i as u16);
        let a = &mut s.actors.attrs;
        a.status[i] = b ^ 1;
        a.strength[i] = b ^ 2;
        a.dexterity[i] = b ^ 3;
        a.intelligence[i] = b ^ 4;
        a.exp[i] = 0x8000 | i as u16;
        a.hp[i] = b ^ 5;
        a.level[i] = b ^ 6;
        a.unknown_10f1[i] = b ^ 7;
        a.combat_mode[i] = b ^ 8;
        a.magic[i] = b ^ 9;
        a.base_type_frame[i] = 0x4321u16.wrapping_add(i as u16);
        a.talk_flags[i] = b ^ 10;
        a.unknown_18f1[i] = b ^ 11;
        a.movement_flags[i] = b ^ 12;
        a.unknown_1af1[i] = b ^ 13;
        s.actors.worktype[i] = b ^ 14;
        s.actors.movement_points[i] = b ^ 15;
    }
    s.objects.obj_status[300] = 0x77;
    let p = &mut s.actors.party;
    p.names = (0..16).map(|i| format!("Name{i}")).collect();
    p.members = std::array::from_fn(|i| i as u8 + 1);
    p.size = 4;
    p.leader_index = 2;
    s.actors.objlist_tail = (0..0x82).map(|i| 0x80 | i as u8).collect();
    s.actors.quest_flag = 0x11;
    s.actors.rest_counter = 0x22;
    s.actors.calendar = crate::actors::Calendar {
        minute: 1,
        hour: 2,
        day: 3,
        month: 4,
        year: 0x0161,
    };
    s.actors.karma = 0x33;
    s.actors.wind = 0x44;
    s.actors.timers = std::array::from_fn(|i| 0x50 + i as u8);
    s.map.centre = Position {
        x: 0x133,
        y: 0x1a7,
        z: 0,
    };
    s
}

#[test]
fn encode_puts_every_table_at_its_offset() {
    let s = filled();
    let f = encode(&s);
    assert_eq!(f.len(), OBJLIST_LEN);
    assert_eq!(f[0x0005], 5, "status");
    assert_eq!(
        &f[0x0100 + 3 * 7..0x0103 + 3 * 7],
        &s.objects.location[7].to_packed()
    );
    assert_eq!(&f[0x0400 + 2 * 9..0x0402 + 2 * 9], &0x123du16.to_be_bytes());
    assert_eq!(&f[0x0600..0x0602], &0xabcdu16.to_be_bytes());
    assert_eq!(f[0x0A00 + 10], 10 ^ 3, "dexterity");
    assert_eq!(&f[0x0C00 + 2..0x0C04], &0x8001u16.to_be_bytes());
    assert_eq!(&f[0x0F00..0x0F06], b"Name0\0");
    assert_eq!(&f[0x0F00 + 14..0x0F00 + 19], b"Name1");
    assert_eq!(f[0x0FE0], 1);
    assert_eq!(f[0x0FF0], 4, "party size");
    assert_eq!(f[0x11F1 + 2], 2 ^ 14, "worktype");
    assert_eq!(f[0x14F1 + 2], 2 ^ 15, "movement points");
    assert_eq!(f[0x1AF1 + 255], 255 ^ 13);
    let t = &f[TAIL..];
    assert_eq!(&t[..10], &[0x11, 0x22, 1, 2, 3, 4, 0x01, 0x61, 0x33, 0x44]);
    assert_eq!(t[0x0a], 2, "leader index");
    assert_eq!(t[0x0b], 0x8b, "undecoded bytes come from the block");
    assert_eq!(
        &t[0x0c..0x12],
        &[0x01, 0x33, 0x01, 0xa7, 0, 0],
        "view centre"
    );
    assert_eq!(t[0x12], 0x50, "timers");
    assert_eq!(t[0x81], 0x81);
}

#[test]
fn decode_reads_back_what_encode_wrote() {
    let s = filled();
    let f = encode(&s);
    let mut t = State::empty();
    decode(&mut t, &f);
    assert_eq!(t.objects.obj_status[..256], s.objects.obj_status[..256]);
    assert_eq!(t.objects.location[..256], s.objects.location[..256]);
    assert_eq!(t.objects.type_frame[..256], s.objects.type_frame[..256]);
    assert_eq!(t.objects.qq[..256], s.objects.qq[..256]);
    assert_eq!(
        t.objects.obj_status[300], 0,
        "only actor slots are in objlist"
    );
    assert_eq!(t.actors.attrs, s.actors.attrs);
    assert_eq!(t.actors.worktype, s.actors.worktype);
    assert_eq!(t.actors.movement_points, s.actors.movement_points);
    assert_eq!(t.actors.party, s.actors.party);
    assert_eq!(t.actors.calendar, s.actors.calendar);
    let a = &t.actors;
    assert_eq!(
        (a.quest_flag, a.rest_counter, a.karma, a.wind),
        (0x11, 0x22, 0x33, 0x44)
    );
    assert_eq!(t.actors.timers, s.actors.timers);
    // The block keeps the file's bytes, decoded ones included.
    assert_eq!(t.actors.objlist_tail, f[TAIL..]);
    // The view centre is not read back here: 0x105350 takes it from the leader.
    t.map.centre = s.map.centre;
    assert_eq!(encode(&t), f);
}

#[test]
fn a_short_file_leaves_the_rest_as_it_was() {
    let s = filled();
    let mut f = encode(&s);
    f.truncate(0x0901);
    let mut t = State::empty();
    t.actors.attrs.strength[5] = 0xee;
    t.actors.attrs.dexterity[5] = 0xdd;
    decode(&mut t, &f);
    assert_eq!(t.actors.attrs.status[5], 5 ^ 1, "read");
    assert_eq!(
        t.actors.attrs.strength[0], 2,
        "first byte of the table read"
    );
    assert_eq!(t.actors.attrs.strength[5], 0xee, "past the end: kept");
    assert_eq!(t.actors.attrs.dexterity[5], 0xdd);
    // Bytes past 0x1C73 are not read.
    let mut long = encode(&s);
    long.extend_from_slice(&[0x5a; 0x100]);
    let mut u = State::empty();
    decode(&mut u, &long);
    u.map.centre = s.map.centre;
    assert_eq!(encode(&u), encode(&s));
}

#[test]
fn an_unchanged_name_keeps_the_bytes_after_its_nul() {
    let mut s = filled();
    let mut f = encode(&s);
    f[0x0F00 + 6..0x0F00 + 14].copy_from_slice(b"garbage!");
    decode(&mut s, &f);
    assert_eq!(s.actors.party.names[0], "Name0");
    assert_eq!(&encode(&s)[0x0F00..0x0F0E], b"Name0\0garbage!");
    s.actors.party.names[0] = "Wxyz".into();
    assert_eq!(&encode(&s)[0x0F00..0x0F0E], b"Wxyz\0\0\0\0\0\0\0\0\0\0");
}

fn block(records: &[[u8; 8]]) -> ObjBlock {
    ObjBlock {
        objects: records.iter().map(|r| ObjRecord::from_bytes(r)).collect(),
    }
}

fn with_blocks() -> State {
    let mut s = State::empty();
    s.objects.blocks = ObjBlockId::all().map(|id| (id, block(&[]))).collect();
    s
}

#[test]
fn commit_replaces_flagged_blocks_only() {
    let mut s = with_blocks();
    s.objects.blocks[5].1 = block(&[[0, 1, 2, 3, 0, 4, 0, 1]]);
    s.objects.blocks[6].1 = block(&[[0, 9, 9, 9, 0, 4, 0, 1]]);
    let rec = [0u8, 7, 0, 0, 0, 0x33, 0, 2];
    let mut tmp = 1u16.to_be_bytes().to_vec();
    tmp.extend_from_slice(&rec);
    s.objects.tmp[5] = Some(tmp);
    s.objects.tmp_flag[5] = 1;
    // A flag without a file: the delete happens, the rename finds nothing.
    s.objects.tmp_flag[7] = 1;
    s.saveload.generation = 3;
    commit_blocks(&mut s);
    assert_eq!(s.objects.blocks[5].1, block(&[rec]));
    assert_eq!(s.objects.blocks[6].1, block(&[[0, 9, 9, 9, 0, 4, 0, 1]]));
    assert_eq!(s.objects.blocks[7].1, block(&[]));
    assert_eq!((s.objects.tmp[5].clone(), s.objects.tmp_flag[5]), (None, 0));
    assert_eq!(s.objects.tmp_flag[7], 0);
    assert_eq!(&s.saveload.block_gen[5..8], &[3, 0, 3]);
}

#[test]
fn a_tmp_file_with_a_count_past_its_records_reads_what_it_holds() {
    let mut s = with_blocks();
    let mut tmp = 3u16.to_be_bytes().to_vec();
    tmp.extend_from_slice(&[0, 1, 0, 0, 0, 2, 0, 0]);
    tmp.push(0xff);
    s.objects.tmp[0] = Some(tmp);
    s.objects.tmp_flag[0] = 1;
    commit_blocks(&mut s);
    assert_eq!(s.objects.blocks[0].1.objects.len(), 1);
}

#[test]
fn changed_since_lists_objlist_and_the_blocks_of_later_saves() {
    let mut s = with_blocks();
    assert!(changed_since(&s, 0).is_empty(), "no save yet");
    s.saveload.generation = 2;
    s.saveload.objlist = Some(vec![1, 2, 3]);
    s.saveload.block_gen[1] = 1;
    s.saveload.block_gen[64] = 2;
    s.objects.blocks[64].1 = block(&[[0, 5, 0, 0x10, 0, 1, 0, 0]]);
    let all: Vec<String> = changed_since(&s, 0).into_keys().collect();
    assert_eq!(
        all,
        ["savegame/objblkAI", "savegame/objblkBA", "savegame/objlist"]
    );
    let later = changed_since(&s, 1);
    assert_eq!(later.len(), 2);
    let mut want = 1u16.to_be_bytes().to_vec();
    want.extend_from_slice(&[0, 5, 0, 0x10, 0, 1, 0, 0]);
    assert_eq!(later["savegame/objblkAI"], want);
    assert!(changed_since(&s, 2).is_empty());
}

#[test]
fn mem_store_replaces_names_without_regard_to_case() {
    let mut store = MemStore::default();
    let mut a = BTreeMap::new();
    a.insert("savegame/OBJBLKAA".to_string(), vec![1]);
    a.insert("savegame/objlist".to_string(), vec![2]);
    store.store(&a).unwrap();
    let mut b = BTreeMap::new();
    b.insert("savegame/objblkAA".to_string(), vec![3]);
    store.store(&b).unwrap();
    let got = store.load().unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got["savegame/objblkAA"], [3]);
}

#[test]
fn attach_lays_the_save_over_the_new_game_files() {
    let mut base = BTreeMap::new();
    base.insert("DATA1/objlist".to_string(), b"new".to_vec());
    base.insert("DATA1/OBJBLKAA".to_string(), b"aa".to_vec());
    base.insert("DATA1/OBJBLKAB".to_string(), b"ab".to_vec());
    let mut files = Files::new(base.clone());
    assert_eq!(attach(&mut files, &MemStore::default()), Ok(0));
    assert_eq!(files.layers().count(), 1, "no save: no layer");
    let mut store = MemStore::default();
    store
        .files
        .insert("savegame/objlist".into(), b"saved".to_vec());
    store
        .files
        .insert("savegame/objblkAA".into(), b"AA2".to_vec());
    let mut files = Files::new(base);
    assert_eq!(attach(&mut files, &store), Ok(2));
    assert_eq!(files.get("savegame/objlist"), Some(&b"saved"[..]));
    assert_eq!(files.get("savegame/objblkaa"), Some(&b"AA2"[..]));
    assert_eq!(
        files.get("savegame/objblkab"),
        Some(&b"ab"[..]),
        "falls through"
    );
}

#[test]
fn boot_objlist_wants_the_bytes_the_game_reads() {
    let mut base = BTreeMap::new();
    base.insert("DATA1/objlist".to_string(), vec![0; OBJLIST_LEN - 1]);
    let err = boot_objlist(&Files::new(base)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "savegame/objlist: 7282 bytes, the game reads 7283"
    );
    let mut base = BTreeMap::new();
    base.insert("DATA1/objlist".to_string(), vec![0; OBJLIST_LEN]);
    assert_eq!(boot_objlist(&Files::new(base)).unwrap().len(), OBJLIST_LEN);
    assert!(boot_objlist(&Files::default()).is_err());
}

/// The three files 0x105350 reads, synthetic: no schedules, base tiles all 0, and `objlist`.
fn state_files(objlist: Vec<u8>) -> Files {
    let mut base = BTreeMap::new();
    base.insert("schedule".to_string(), vec![0; 514]);
    base.insert("basetile".to_string(), vec![0; 0x800]);
    base.insert("DATA1/objlist".to_string(), objlist);
    Files::new(base)
}

/// Restore (0x1052c8) of `objlist` on an empty state (empty block files unless `s` has some),
/// then `after` on the restored state.
fn restored(
    mut s: State,
    objlist: Vec<u8>,
    after: impl FnOnce(&mut State) + 'static,
) -> crate::engine::Engine {
    if s.objects.blocks.is_empty() {
        s.objects.blocks = ObjBlockId::all().map(|id| (id, block(&[]))).collect();
    }
    let mut e = crate::engine::Engine::with_game(s, state_files(objlist), |st, io| async move {
        let mut s = st.borrow_mut();
        restore(&mut s, &io).expect("the synthetic files parse");
        after(&mut s);
    });
    for _ in 0..20 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    e
}

#[test]
fn a_party_size_past_the_roster_is_clamped_and_the_hour_passes() {
    // 0x112daa refuses a ninth member; a file with 17 must not index past the 16 slots.
    let mut f = encode(&State::empty());
    f[0x0FF0] = 17;
    f[0x0FE0..0x0FF0].copy_from_slice(&[1; 16]);
    let e = restored(State::empty(), f, |s| crate::actors::clock::tick(s, 60));
    let s = e.state();
    assert_eq!(s.actors.party.size, 15, "clamped at the vessel slot");
    assert_eq!(s.actors.calendar.hour, 1, "an hour on");
}

#[test]
fn a_booted_party_size_past_the_roster_is_clamped_and_the_hour_passes() {
    // The boot's way in (no Restore): the objlist steps of `State::new_game` (the boot's file,
    // decode, the view on the leader), main's load-state tail, 0x1217d4 and the clock.
    let mut f = encode(&State::empty());
    f[0x0FF0] = 17;
    f[0x0FE0..0x0FF0].copy_from_slice(&[1; 16]);
    let files = state_files(f);
    let mut s = State::empty();
    s.objects.blocks = ObjBlockId::all().map(|id| (id, block(&[]))).collect();
    decode(&mut s, boot_objlist(&files).expect("synthetic objlist"));
    s.place_party();
    crate::main_loop::load_state_tail(&mut s);
    crate::actors::turn::after_load(&mut s);
    crate::actors::clock::tick(&mut s, 0);
    assert_eq!(s.actors.party.size, 15, "clamped at the vessel slot");
    crate::actors::clock::tick(&mut s, 60);
    assert_eq!(s.actors.calendar.hour, 1, "an hour on");
}

#[test]
fn restore_leaves_the_save_mode_at_play() {
    // 0x1052c8 sets A4-0x637a = 0x20 before 0x105350, which does not write it.
    let mut s = State::empty();
    s.objects.defs.save_mode = 0x52;
    s.objects.save_mode = 0x47;
    let e = restored(s, encode(&State::empty()), |_| {});
    assert_eq!(e.state().objects.save_mode, 0x20);
}

#[test]
fn restore_reads_the_region_in_play_mode() {
    // 0x1052c8 sets A4-0x637a = 0x20 before 0x105350, so the region's block reads run in play
    // mode whatever the data hunk started it at (0x10ddd2 reads nothing in mode 'R').
    let mut s = State::empty();
    s.objects.defs.save_mode = 0x52;
    let rec = [[0u8, 1, 0, 0, 0, 0x77, 0, 1]];
    s.objects.blocks = ObjBlockId::all()
        .enumerate()
        .map(|(i, id)| (id, block(if i < 64 { &rec } else { &[] })))
        .collect();
    let e = restored(s, encode(&State::empty()), |_| {});
    let o = &e.state().objects;
    let loaded = (crate::objects::ACTOR_SLOTS..crate::objects::OBJECT_SLOTS)
        .filter(|&i| o.type_frame[i] & 0x3ff == 0x77)
        .count();
    assert!(loaded > 0, "the region's objects in slots: {loaded}");
}

#[test]
fn attach_takes_only_the_files_the_game_reads() {
    let mut base = BTreeMap::new();
    base.insert("DATA1/schedule".to_string(), b"s".to_vec());
    let mut store = MemStore::default();
    for (name, bytes) in [
        ("SAVEGAME/OBJLIST", &b"saved"[..]),
        ("savegame/objblkHI", b"HI"),
        ("savegame/objblkEI", b"EI"),
        ("savegame/objblkAA.tmp", b"tmp"),
        ("schedule", b"stray"),
        ("savegame/notes", b"n"),
    ] {
        store.files.insert(name.into(), bytes.to_vec());
    }
    let mut files = Files::new(base);
    assert_eq!(attach(&mut files, &store), Ok(2), "objlist and objblkEI");
    assert_eq!(files.get("savegame/objlist"), Some(&b"saved"[..]));
    assert_eq!(files.get("savegame/objblkei"), Some(&b"EI"[..]));
    assert_eq!(files.get("schedule"), Some(&b"s"[..]), "not overlaid");
    assert_eq!(files.get("savegame/objblkHI"), None, "no block 71");
    assert_eq!(files.get("savegame/notes"), None);
}

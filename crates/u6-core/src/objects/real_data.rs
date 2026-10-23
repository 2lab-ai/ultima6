//! Real data: the user's archive.org zip → u6-intake → `Files` → `State::new_game`, then the
//! object code. Skips when the zip is absent; reads only and prints nothing but the skip line.
//! A failure names its label, a block index or an object slot, and the index where two
//! sequences first differ (`assert_same!`), never a name, string, count or length of the game's.
#![allow(clippy::disallowed_methods)] // the test reads the zip

use std::collections::BTreeMap;
use std::path::Path;

use super::iter::{flags2, tile_of};
use super::*;
use crate::files::Files;
use crate::state::State;
use u6_formats::objblk::ObjLocation;
use u6_testkit::Quiet;
use u6_testkit::assert_same;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

fn files() -> Option<Files> {
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

/// The state after load: tables, objlist, the object tables from `game`, slots reset and
/// actors linked (no region yet).
fn loaded(files: &Files) -> State {
    let mut s = State::new_game(files).quiet();
    s.objects.load_defs(files).quiet();
    let o = &mut s.objects;
    init_slots(o);
    link_actors(o);
    s
}

#[test]
fn every_block_loads_through_the_slot_tables() {
    let Some(files) = files() else { return };
    let mut s = loaded(&files);
    // 0x10cef2 compares with 21 table words (the loop bound is the code's).
    assert!(s.objects.defs.stackable.len() == 21);
    // What the block files hold, read from the raw files (formats.md §5: a BE record count, then
    // 8-byte records whose byte 0 has the location in bits 3-4).
    let mut raw_total = 0usize;
    let mut raw_hist: BTreeMap<u8, usize> = BTreeMap::new();
    for i in 0..BLOCK_COUNT {
        let raw = files
            .get(&crate::saveload::block_file(i))
            .expect("the block file");
        let n = usize::from(u16::from_be_bytes([raw[0], raw[1]]));
        for k in 0..n {
            *raw_hist.entry(raw[2 + 8 * k] & 0x18).or_default() += 1;
        }
        raw_total += n;
    }
    // The strings came out of the data hunk (shape only; the text is the user's).
    let d = &s.objects.defs;
    assert!(d.nothing.starts_with(|c: char| c.is_ascii_uppercase()));
    assert_same!(
        d.nothing.to_ascii_lowercase(),
        d.nothing_lower,
        "the lower-case nothing word"
    );
    assert!(d.count_format.contains("%d"));
    let mut hist: BTreeMap<u8, usize> = BTreeMap::new();
    let mut total = 0;
    for i in 0..BLOCK_COUNT {
        let before = s.objects.free.1;
        let file_count = s.objects.blocks[i].1.objects.len();
        load_block(&mut s.objects, i);
        s.objects.fix_inventory();
        let n = (before - s.objects.free.1) as usize;
        assert!(n == file_count, "block {i}: every record took a slot");
        for k in 0..n {
            let slot = s.objects.rec_slots[k];
            *hist.entry(s.objects.status(slot) & WHERE_MASK).or_default() += 1;
        }
        total += n;
        write_back_block(&mut s, i);
        let tmp = s.objects.tmp[i].clone().quiet();
        assert!(
            u16::from_be_bytes([tmp[0], tmp[1]]) as usize == n,
            "block {i} wrote back"
        );
    }
    assert!(total == raw_total, "every record of the files");
    let by_place = |h: &BTreeMap<u8, usize>| h.iter().map(|(&k, &n)| (k, n)).collect::<Vec<_>>();
    assert_same!(
        by_place(&hist),
        by_place(&raw_hist),
        "the records by location bits"
    );
    // 0x10ed44's free count (an immediate of the code).
    assert!(s.objects.free.1 == 0xaff, "every slot came back");
}

#[test]
fn the_party_start_region_and_its_objects() {
    let Some(files) = files() else { return };
    let mut s = loaded(&files);
    let at = s.leader_position().quiet();
    s.objects.set_window(
        crate::geom::window_origin(at.x),
        crate::geom::window_origin(at.y),
        at.z,
    );
    load_region(&mut s, at.x, at.y, u16::from(at.z));
    let mut loaded: Vec<i16> = s.objects.loaded_surface.to_vec();
    loaded.sort();
    assert!(loaded.iter().all(|&b| b >= 0));
    s.objects.rebuild_grid();

    // Expected, from the raw files: map records and actors on the tile, and the multi-tile
    // ones one tile east / south / south-east that cover it.
    let t = &s.tables;
    let covers = |obj_n: u16, frame: u8, dx: u16, dy: u16| {
        let f2 = flags2(t, tile_of(t, obj_n | u16::from(frame) << 10));
        match (dx, dy) {
            (0, 0) => true,
            (1, 0) => f2 & 0x80 != 0,
            (0, 1) => f2 & 0x40 != 0,
            _ => f2 & 0xc0 == 0xc0,
        }
    };
    let mut want: Vec<(u16, u8)> = Vec::new();
    for (_, b) in &s.objects.blocks {
        for r in &b.objects {
            let (dx, dy) = (r.pos.x.wrapping_sub(at.x), r.pos.y.wrapping_sub(at.y));
            let on_map = r.location() == ObjLocation::Map && r.pos.z == at.z;
            if on_map && dx <= 1 && dy <= 1 && covers(r.obj_n, r.frame, dx, dy) {
                want.push((r.obj_n, r.frame));
            }
        }
    }
    for a in 0..ACTOR_SLOTS as Slot {
        let o = &s.objects;
        let (dx, dy) = (o.x(a).wrapping_sub(at.x), o.y(a).wrapping_sub(at.y));
        let frame = (o.tf(a) >> 10) as u8;
        let near = o.tf(a) != 0 && o.z(a) == at.z && dx <= 1 && dy <= 1;
        if near && covers(o.kind(a), frame, dx, dy) {
            want.push((o.kind(a), frame));
        }
    }
    let got: Vec<(u16, u8)> = objects_at(&mut s, at)
        .into_iter()
        .map(|(id, _)| (s.objects.obj_type(id), s.objects.frame(id)))
        .collect();
    let (mut g, mut w) = (got.clone(), want.clone());
    g.sort();
    w.sort();
    assert_same!(g, w, "the objects at the party's start");
    assert!(got.contains(&(
        s.objects.obj_type(ObjectId(1)),
        s.objects.frame(ObjectId(1))
    )));
}

#[test]
fn names_of_twenty_objects_near_the_party() {
    let Some(files) = files() else { return };
    let mut s = loaded(&files);
    let at = s.leader_position().quiet();
    load_region(&mut s, at.x, at.y, u16::from(at.z));
    // The first 10 visible stacks of more than one item (plurals) in list order, then visible
    // map objects from the party's row on.
    let (mut stacks, mut near) = (Vec::new(), Vec::new());
    let mut c = s.objects.next_of(HEAD);
    while c >= 0 {
        let o = &s.objects;
        // Eggs (0x14f) count as stacks but are never drawn (0x10e596 skips their tile).
        let visible = o.status(c) & 0x02 == 0 && c >= ACTOR_SLOTS as Slot && o.kind(c) != 0x14f;
        if visible && stack_kind(o, o.kind(c)) != 0 && quantity(o, c) > 1 && stacks.len() < 10 {
            stacks.push(c);
        } else if visible && o.place(c) == 0 && o.y(c) >= at.y && near.len() < 10 {
            near.push(c);
        }
        c = o.next_of(c);
    }
    let picks: Vec<Slot> = stacks.into_iter().chain(near).collect();
    assert!(picks.len() == 20);
    let t = &s.tables;
    for &c in &picks {
        let o = &s.objects;
        let tile = tile_of(t, o.tf(c));
        let (prefix, count) = name_prefix_text(&s, c);
        let d = &o.defs;
        let count_text = |n: u16| d.count_format.replace("%d", &n.to_string());
        let want_prefix = match stack_kind(o, o.kind(c)) {
            4 => count_text(o.qq_of(c)),
            2 if o.status(c) & 0x80 == 0 => count_text(o.qq_of(c) & 0xff),
            _ => match t.tileflags.article(usize::from(tile)) {
                Some(u6_formats::tileflag::Article::A) => d.articles[0].clone(),
                Some(u6_formats::tileflag::Article::An) => d.articles[1].clone(),
                Some(u6_formats::tileflag::Article::The) => d.articles[2].clone(),
                _ => String::new(),
            },
        };
        let want = format!(
            "{want_prefix}{}",
            t.look.description(tile, count != 1).quiet()
        );
        let got = described_name(&s, c);
        assert!(got.starts_with(&prefix));
        assert_same!(got, want, "the name of slot {c:#05x}");
    }
}

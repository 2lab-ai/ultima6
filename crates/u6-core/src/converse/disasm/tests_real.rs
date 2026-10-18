//! Real data (owner-local): the disassembler over every script of the owner's `converse.a` and
//! `converse.b`, and the execution cross-check on the real-data conversations of
//! `converse/tests_real.rs`. Skips where the zip is missing; prints counts, hashes and
//! pass/fail only, never a script's bytes, text or names.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::collections::BTreeMap;

use u6_formats::converse::ConverseLib;
use u6_testkit::Quiet;

use super::super::party::name_from_script;
use super::super::tests_real::{converse, files, with_text};
use super::catalog::{Catalog, hash64};
use super::trace::{self, Counts, Decoded};
use super::{Disasm, Kind, Operands, disassemble};
use crate::files::Files;
use crate::objects::ObjectId;

/// Every non-empty script of both archives: (letter, item, bytes).
fn scripts(files: &Files) -> Vec<(u8, u16, Vec<u8>)> {
    let mut out = Vec::new();
    for letter in [b'a', b'b'] {
        let name = format!("converse.{}", char::from(letter));
        let lib = ConverseLib::parse(files.get(&name).quiet()).quiet();
        for (item, s) in lib.scripts.into_iter().enumerate() {
            if let Some(s) = s {
                out.push((letter, item as u16, s.bytecode));
            }
        }
    }
    out
}

/// Every byte of the script in exactly one item, in order.
fn covers_once(d: &Disasm) -> bool {
    let mut at = 0;
    for it in &d.items {
        if it.offset != at || it.len == 0 {
            return false;
        }
        at = it.end();
    }
    at == d.len
}

#[test]
fn every_script_disassembles_with_every_byte_in_one_item() {
    let Some(files) = files() else { return };
    let all = scripts(&files);
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut hazards: BTreeMap<String, usize> = BTreeMap::new();
    let (mut decoded, mut bytes, mut once, mut unreached) = (0, 0usize, 0usize, 0usize);
    let (mut strings, mut tables) = (0, 0);
    for (letter, item, b) in &all {
        let d = match disassemble(*letter, *item, b) {
            Ok(d) => d,
            Err(r) => {
                *refused.entry(r.reason.to_string()).or_default() += 1;
                continue;
            }
        };
        decoded += 1;
        bytes += b.len();
        assert!(covers_once(&d), "script {}{item}: coverage", char::from(*letter));
        once += d.items.iter().map(|i| i.len as usize).sum::<usize>();
        for it in &d.items {
            let k = match &it.kind {
                Kind::Header => "header",
                Kind::Name => "name",
                Kind::Marker(_) => "marker",
                Kind::Text(_) => "run",
                Kind::Op(op) if matches!(op.operands, Operands::Jump { .. }) => "op (jump)",
                Kind::Op(_) => "op",
                Kind::Keywords(_) => "keyword block",
                Kind::BlockEnd => "block end",
                Kind::Input(_) => "input op",
                Kind::Stop(_) => "stop",
                Kind::Table(t) => {
                    tables += 1;
                    if !t.words {
                        strings += t.entries.len();
                    }
                    "table"
                }
                Kind::Unreached => {
                    unreached += it.len as usize;
                    "unreached range"
                }
            };
            *kinds.entry(k).or_default() += 1;
        }
        for h in &d.hazards {
            let k = format!("{:?}", h.kind);
            *hazards.entry(k).or_default() += 1;
        }
    }
    let catalog = Catalog::build(all.iter().map(|(l, i, b)| (*l, *i, b.as_slice())));
    let runs: usize = catalog.scripts.iter().map(|s| s.runs.len()).sum();
    println!(
        "scripts {} decoded {decoded} refused {}",
        all.len(),
        all.len() - decoded
    );
    for (r, n) in &refused {
        println!("refused: {r} x{n}");
    }
    println!("items by kind: {kinds:?}");
    println!("runs {runs}, table strings {strings} in {tables} tables, unreached bytes {unreached}");
    println!("hazards: {hazards:?}");
    println!(
        "bytes {bytes}, covered by exactly one item {once} ({}%)",
        if bytes == 0 { 0 } else { once * 100 / bytes }
    );
    println!(
        "catalog fnv1a64 {:016x}",
        hash64(catalog.to_json().as_bytes())
    );
    assert_eq!(once, bytes);
    assert!(decoded > 0);
}

#[test]
fn the_real_conversations_read_only_what_the_disassembler_found() {
    let Some(files) = files() else { return };
    let all = scripts(&files);
    let disasms: Vec<(u8, u16, &[u8], Disasm)> = all
        .iter()
        .filter_map(|(l, i, b)| Some((*l, *i, b.as_slice(), disassemble(*l, *i, b).ok()?)))
        .collect();
    let decoded: BTreeMap<(u8, u16), Decoded<'_>> = disasms
        .iter()
        .map(|(l, i, b, d)| {
            (
                (*l, *i),
                Decoded {
                    bytes: b,
                    disasm: d,
                },
            )
        })
        .collect();
    let base = with_text(&files);
    // The conversations of `three_npcs_near_the_start_answer_name_job_bye`: the 40 actors
    // nearest the party's leader that have a script name.
    let leader = base.leader_position().quiet();
    let members: Vec<u16> = base.actors.party.active().map(|id| id.0).collect();
    let mut near: Vec<(u32, i16)> = (2..0xe0i16)
        .filter(|a| !members.contains(&(*a as u16)))
        .filter_map(|a| {
            let p = base.objects.location(ObjectId(a as u16))?;
            if p.z != leader.z || base.objects.obj_type(ObjectId(a as u16)) == 0 {
                return None;
            }
            let d = (i32::from(p.x) - i32::from(leader.x)).unsigned_abs()
                + (i32::from(p.y) - i32::from(leader.y)).unsigned_abs();
            Some((d, a))
        })
        .collect();
    near.sort();
    let mut total = Counts::default();
    let mut talked = 0;
    for &(_, npc) in near.iter().take(40) {
        let mut s = base.clone();
        let (name, _) = name_from_script(&mut s, &files, npc);
        if name.is_empty() {
            continue;
        }
        trace::start(true);
        let _ = converse(&base, &files, npc, &["name", "job", "bye"]);
        let events = trace::stop();
        let n = trace::check(&events, &decoded);
        if n.failures() > 0 {
            println!("actor slot {npc:#x}: {n:?}");
        }
        total.add(&n);
        talked += 1;
    }
    println!("conversations {talked}");
    println!(
        "offsets checked {}, visits filled {}, mismatches {} (bytes {}, partial visits {}, fill {}, table {}, past the item {}, unknown script {})",
        total.offsets,
        total.filled,
        total.mismatches,
        total.byte_mismatches,
        total.partial_visits,
        total.fill_mismatches,
        total.table_mismatches,
        total.past_item,
        total.unknown_script
    );
    assert!(total.offsets > 0 && total.filled > 0);
    assert_eq!(total.failures(), 0, "cross-check mismatches");
}

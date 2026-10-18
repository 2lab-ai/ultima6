//! Real data (owner-local): the disassembler over every script of the owner's `converse.a` and
//! `converse.b`, and the execution cross-check on the real-data conversations of
//! `converse/tests_real.rs`, then on the same conversations asked every keyword of their
//! scripts. Skips where the zip is missing; prints counts, kinds, hashes and pass/fail only,
//! never a script's bytes, text, names or keywords.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::collections::{BTreeMap, BTreeSet};

use u6_formats::converse::ConverseLib;
use u6_testkit::Quiet;

use super::super::party::name_from_script;
use super::super::script::{ScriptDst, load_script};
use super::super::tests_real::{converse, files, with_text};
use super::super::{Cx, Msg, talk};
use super::catalog::{Catalog, hash64};
use super::trace::{self, Counts, Decoded, Event};
use super::{Disasm, Kind, Operands, disassemble};
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::objects::ObjectId;
use crate::state::State;

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

/// Every byte of the input in exactly one item, in order.
fn covers_once(d: &Disasm, input: &[u8]) -> bool {
    assert_eq!(
        d.len as usize,
        input.len(),
        "the decode's length is the input's"
    );
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
    let (mut strings, mut tables, mut splits) = (0, 0, 0);
    // Splits whose second part is one newline byte / longer (the run is cut in its text).
    let (mut newline_tails, mut mid_text) = (0, 0);
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
        splits += d.run_splits.len();
        for &at in &d.run_splits {
            let part = d.items.iter().find(|i| i.offset == at);
            match part.map(|p| (p.len, b[at as usize])) {
                Some((1, b'\n')) => newline_tails += 1,
                Some((l, _)) if l > 1 => mid_text += 1,
                _ => {}
            }
        }
        assert!(
            covers_once(&d, b),
            "script {}{item}: coverage",
            char::from(*letter)
        );
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
    let linked = catalog
        .scripts
        .iter()
        .flat_map(|s| &s.runs)
        .filter(|r| r.part_of.is_some())
        .count();
    let slotted = catalog
        .scripts
        .iter()
        .flat_map(|s| &s.strings)
        .filter(|s| !s.slots.is_empty())
        .count();
    let ops = kinds.get("op").copied().unwrap_or(0) + kinds.get("op (jump)").copied().unwrap_or(0);
    let data = kinds.get("table").copied().unwrap_or(0)
        + kinds.get("unreached range").copied().unwrap_or(0);
    println!(
        "scripts {} decoded {decoded} refused {}",
        all.len(),
        all.len() - decoded
    );
    for (r, n) in &refused {
        println!("refused: {r} x{n}");
    }
    println!("items by kind: {kinds:?}");
    println!(
        "runs {runs} ({linked} of them parts of a split run), table strings {strings} ({slotted} copied into a slot), ops {ops}, data ranges {data} ({tables} tables, unreached bytes {unreached})"
    );
    println!(
        "run splits {splits} (a one-byte newline tail {newline_tails}, a cut inside the text {mid_text}), hazards: {hazards:?}"
    );
    println!(
        "bytes {bytes}, covered by exactly one item {once} ({}%)",
        (once * 100).checked_div(bytes).unwrap_or(0)
    );
    println!(
        "catalog fnv1a64 {:016x}",
        hash64(catalog.to_json().as_bytes())
    );
    assert!(refused.is_empty(), "refused: {refused:?}");
    assert!(decoded > 0 && decoded == all.len(), "every script decodes");
    assert_eq!(once, bytes);
    let back = Catalog::from_json(&catalog.to_json());
    assert!(back.is_ok_and(|c| c == catalog), "the catalog reads back");
}

/// The (archive, item, offset) of each table string `events` read from a table address.
fn table_reads(events: &[Event], strings: &mut BTreeSet<(u8, u16, i32)>) {
    let (mut script, mut last, mut outside) = (None, None, false);
    for e in events {
        match e {
            Event::Load { letter, item } => {
                (script, last, outside) = (Some((*letter, *item)), None, false);
            }
            Event::Run { .. } | Event::Input { .. } => (last, outside) = (None, false),
            Event::Address { inside } => outside = !inside,
            Event::Table { at, byte } if !outside => {
                if let Some((l, i)) = script
                    && last.is_none()
                {
                    strings.insert((l, i, *at));
                }
                last = (*byte != 0).then_some(*at);
            }
            Event::Emit(_) | Event::Table { .. } => {}
            Event::Word { .. } | Event::TableEnd { .. } | Event::WordWrite { .. } => last = None,
        }
    }
}

/// The goodbye typed at the end, three times: a question after it can bring the prompt back.
const BYE: &str = "bye";

/// What the message scroll shows: its lines and the one being written.
fn screen(s: &State) -> (usize, Vec<u8>) {
    let t = &s.ui.text;
    let mut last = t.transcript_line.clone();
    last.extend(&t.line[usize::from(t.line_start)..usize::from(t.line_len)]);
    (s.ui.messages.len(), last)
}

/// Talks to `npc` as `converse` does, typing the next of `lines` at each prompt for the
/// player's words, and at any other wait trying space, N, Y and the digits in turn (a digit
/// key answers `fc`), on through the keys while the screen changes (a list that waits for a key
/// after each line ends on one of them). Whether the conversation ended.
fn talk_through(base: &State, files: &Files, npc: i16, lines: &[&str]) -> bool {
    let you_say = base.converse.text.as_ref().quiet().get(Msg::YouSay);
    let prompt: Vec<u8> = you_say.trim_ascii().to_vec();
    let leader = base.actors.party.leader_index;
    let mut e = Engine::with_game(base.clone(), files.clone(), move |st, io| async move {
        talk(&Cx::new(&st, &io), leader, npc, true).await;
    });
    let mut lines = lines.iter();
    let mut tries = 0;
    for _ in 0..400 {
        for _ in 0..32 {
            let before = screen(&e.state());
            e.tick(&[], 16);
            if e.finished() || (e.io().pending_events() == 0 && screen(&e.state()) == before) {
                break;
            }
        }
        if e.finished() {
            return true;
        }
        let shown = screen(&e.state()).1;
        let asked = e.state().ui.input.line_input || shown.trim_ascii_end().ends_with(&prompt);
        let keys: Vec<InputEvent> = if asked {
            let Some(line) = lines.next() else {
                return false;
            };
            tries = 0;
            line.chars()
                .map(|c| InputEvent::Key(Key::Char(c)))
                .chain([InputEvent::Key(Key::Enter)])
                .collect()
        } else {
            tries += 1;
            const KEYS: &[u8] = b" ny1234567890";
            let k = char::from(KEYS[(tries - 1) % KEYS.len()]);
            vec![InputEvent::Key(Key::Char(k))]
        };
        e.tick(&keys, 16);
    }
    e.finished()
}

#[test]
fn the_real_conversations_read_only_what_the_disassembler_found() {
    let Some(files) = files() else { return };
    let all = scripts(&files);
    let disasms: Vec<(u8, u16, &[u8], Disasm)> = all
        .iter()
        .map(|(l, i, b)| {
            let d = disassemble(*l, *i, b)
                .unwrap_or_else(|r| panic!("script {}{i}: {}", char::from(*l), r.reason));
            (*l, *i, b.as_slice(), d)
        })
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
    let bye: String = base
        .converse
        .text
        .as_ref()
        .quiet()
        .get(Msg::Bye)
        .trim_ascii()
        .iter()
        .map(|&c| char::from(c))
        .collect();
    // The conversations of `three_npcs_near_the_start_answer_name_job_bye`: the 40 actors
    // nearest the party's leader (those with a script name are talked to).
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
    let mut npcs: Vec<i16> = near.iter().take(40).map(|&(_, a)| a).collect();
    // And the conversation of `greeting_substitutes_title_time_and_names`: the first actor
    // whose script uses `$G` and `$T`.
    let mut s = base.clone();
    let greeter = (2..0xe0i16).find(|&npc| {
        let mut buf = Vec::new();
        load_script(&mut s, &files, npc, ScriptDst::Other(&mut buf)) != 0
            && buf.windows(2).any(|w| w == b"$G")
            && buf.windows(2).any(|w| w == b"$T")
    });
    assert!(greeter.is_some(), "an actor's script greets with $G and $T");
    npcs.extend(greeter);
    let mut total = Counts::default();
    let (mut talked, mut nameless) = (0, 0);
    let mut read = BTreeSet::new();
    // Each conversation's script, from its load, for the keyword pass.
    let mut script_of: Vec<(i16, (u8, u16))> = Vec::new();
    // `converse` for the first pass (its conversations, each must end); `talk_through` for
    // the keywords, where an answer may wait on a key no generic driver gives: a conversation
    // that does not end is checked up to its last read of the input loop (the run it is in is
    // not cut, only not finished).
    let talk = |npc: i16,
                lines: &[&str],
                keywords: bool,
                total: &mut Counts,
                read: &mut BTreeSet<(u8, u16, i32)>| {
        trace::start(true);
        let done = if keywords {
            talk_through(&base, &files, npc, lines)
        } else {
            converse(&base, &files, npc, lines).1
        };
        let mut events = trace::stop();
        assert!(
            done || keywords,
            "actor slot {npc:#x}: the conversation ends"
        );
        if !done {
            let cut = events
                .iter()
                .rposition(|e| matches!(e, Event::Input { .. }));
            events.truncate(cut.map_or(0, |i| i + 1));
        }
        let n = trace::check(&events, &decoded);
        if n.failures() > 0 {
            println!("actor slot {npc:#x}: {n:?}");
        }
        total.add(&n);
        table_reads(&events, read);
        let loaded = events.iter().find_map(|e| match e {
            Event::Load { letter, item } => Some((*letter, *item)),
            _ => None,
        });
        (loaded, done)
    };
    for &npc in &npcs {
        let mut s = base.clone();
        let (name, _) = name_from_script(&mut s, &files, npc);
        if name.is_empty() {
            nameless += 1;
            continue;
        }
        let lines = ["name", "job", BYE, BYE, BYE];
        let (loaded, _) = talk(npc, &lines, false, &mut total, &mut read);
        script_of.push((npc, loaded.expect("the conversation loads its script")));
        talked += 1;
    }
    println!("conversations {talked} (actors without a script name {nameless})");
    print_counts("name, job, bye", &total, read.len());
    let first = total.clone();
    // Every actor with a script name (not in the party) asked every keyword of its script, a
    // few per conversation, then the goodbye: the answers print and copy table strings.
    let mut s = base.clone();
    for npc in 2..0xe0i16 {
        if members.contains(&(npc as u16)) || script_of.iter().any(|&(a, _)| a == npc) {
            continue;
        }
        if name_from_script(&mut s, &files, npc).0.is_empty() {
            continue;
        }
        trace::start(false);
        load_script(&mut s, &files, npc, ScriptDst::Main);
        let loaded = trace::stop().iter().find_map(|e| match e {
            Event::Load { letter, item } => Some((*letter, *item)),
            _ => None,
        });
        script_of.extend(loaded.map(|key| (npc, key)));
    }
    let (mut asked, mut ended) = (0, 0);
    for &(npc, key) in &script_of {
        let mut keys: BTreeSet<String> = BTreeSet::new();
        for it in &decoded[&key].disasm.items {
            if let Kind::Keywords(spans) = &it.kind {
                for sp in spans {
                    let k: String = sp
                        .bytes(decoded[&key].bytes)
                        .iter()
                        .map(|&b| if b == b'?' { 'a' } else { char::from(b) })
                        .collect();
                    if !k.is_empty() && !k.starts_with('*') && !k.eq_ignore_ascii_case(&bye) {
                        keys.insert(k);
                    }
                }
            }
        }
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        for chunk in keys.chunks(6) {
            let mut lines = chunk.to_vec();
            lines.extend([BYE; 3]);
            ended += usize::from(talk(npc, &lines, true, &mut total, &mut read).1);
            asked += 1;
        }
    }
    println!(
        "keyword conversations {asked} (ended {ended}) with {} actors",
        script_of.len()
    );
    let catalog_strings: usize = disasms
        .iter()
        .flat_map(|(.., d)| &d.items)
        .map(|it| match &it.kind {
            Kind::Table(t) if !t.words => t.entries.len(),
            _ => 0,
        })
        .sum();
    print_counts("all", &total, read.len());
    assert!(first.offsets > 0 && first.filled > 0);
    println!(
        "distinct table strings read {} of {catalog_strings}",
        read.len()
    );
    assert!(talked > 0 && total.offsets > 0 && total.filled > 0);
    assert!(
        total.table_strings > 0,
        "the conversations read table strings"
    );
    assert_eq!(total.failures(), 0, "cross-check mismatches");
}

fn print_counts(pass: &str, n: &Counts, distinct: usize) {
    println!(
        "{pass}: offsets checked {}, visits filled {}, table strings read {} ({distinct} distinct), table words read {}, table ends {}, table words written {}, table ops with an address outside the stack {}, mismatches {} (bytes {}, partial visits {}, fill {}, table {}, past the item {}, unknown script {})",
        n.offsets,
        n.filled,
        n.table_strings,
        n.table_words,
        n.table_ends,
        n.table_writes,
        n.outside_reads,
        n.mismatches,
        n.byte_mismatches,
        n.partial_visits,
        n.fill_mismatches,
        n.table_mismatches,
        n.past_item,
        n.unknown_script
    );
}

//! Real data (owner-local): the disassembler over every script of the owner's `converse.a` and
//! `converse.b`, and the execution cross-check on the real-data conversations of
//! `converse/tests_real.rs`, then with every actor asked every keyword of its script. Skips where
//! the zip is missing. Its output, passing or failing, holds only counts of what the test made
//! (scripts decoded and refused, items and hazards by kind, conversations, keywords sent,
//! checks and mismatches), labels, actor slots and pass/fail (spec 01): never a value or a length
//! of the owner's data, a byte, a text, a name, a keyword or a hash of them.
#![allow(clippy::disallowed_methods)] // tests may read files

use std::collections::{BTreeMap, BTreeSet};

use u6_formats::converse::ConverseLib;
use u6_testkit::Quiet;

use super::super::party::name_from_script;
use super::super::script::{ScriptDst, load_script};
use super::super::tests_real::{converse, files, with_text};
use super::super::{Cx, Msg, talk};
use super::catalog::Catalog;
use super::trace::{self, Counts, Decoded, Event};
use super::{Disasm, HazardKind, Kind, Operands, Reason, disassemble};
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

/// A refusal's kind, without the byte it names.
fn reason_label(r: Reason) -> &'static str {
    match r {
        Reason::TooLong => "too long",
        Reason::NoHeader => "no header",
        Reason::NoLookMarker => "no look marker",
        Reason::NoGreetingMarker => "no greeting marker",
        Reason::Truncated => "truncated",
        Reason::PastEnd => "past the end",
        Reason::UnknownOp(_) => "unknown op",
        Reason::Malformed(_) => "malformed operand",
        Reason::JumpOutside => "jump outside",
        Reason::Overlap => "overlap",
        Reason::TableOutside => "table outside",
        Reason::TableInCode => "table in code",
        Reason::TableNotLiteral => "table not a literal",
        Reason::TableKinds => "table kinds",
        Reason::TableIndex => "table index",
    }
}

/// A hazard's kind, its refusal reason without the byte it names.
fn hazard_label(k: HazardKind) -> String {
    match k {
        HazardKind::Dropped(scan, r) => format!("Dropped({scan:?}, {})", reason_label(r)),
        k => format!("{k:?}"),
    }
}

/// Every byte of the input in exactly one item, in order.
fn covers_once(d: &Disasm, input: &[u8]) -> bool {
    assert!(
        d.len as usize == input.len(),
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
    let (mut decoded, mut bytes, mut once) = (0, 0usize, 0usize);
    let (mut strings, mut tables, mut splits) = (0, 0, 0);
    // Splits whose second part is one newline byte / longer (the run is cut in its text).
    let (mut newline_tails, mut mid_text) = (0, 0);
    for (letter, item, b) in &all {
        let d = match disassemble(*letter, *item, b) {
            Ok(d) => d,
            Err(r) => {
                *refused
                    .entry(reason_label(r.reason).to_string())
                    .or_default() += 1;
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
                Kind::Unreached => "unreached range",
            };
            *kinds.entry(k).or_default() += 1;
        }
        for h in &d.hazards {
            let k = hazard_label(h.kind);
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
        "runs {runs} ({linked} of them parts of a split run), table strings {strings} ({slotted} copied into a slot), ops {ops}, data ranges {data} ({tables} tables)"
    );
    println!(
        "run splits {splits} (a one-byte newline tail {newline_tails}, a cut inside the text {mid_text}), hazards: {hazards:?}"
    );
    assert!(refused.is_empty(), "refused: {refused:?}");
    assert!(decoded > 0 && decoded == all.len(), "every script decodes");
    assert!(once == bytes, "every byte in exactly one item");
    println!("every byte in exactly one item: pass");
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
            Event::Table { at, byte, .. } if !outside => {
                if let Some((l, i)) = script
                    && last.is_none()
                {
                    strings.insert((l, i, *at));
                }
                last = (*byte != 0).then_some(*at);
            }
            Event::Emit { .. } | Event::Table { .. } | Event::PrintStart { .. } => {}
            Event::Word { .. } | Event::TableEnd { .. } | Event::WordWrite { .. } => last = None,
        }
    }
}

/// The goodbye typed at the end, three times: a question after it can bring the prompt back.
const BYE: &str = "bye";

/// The rounds `talk_through` gives a conversation: each one runs the game until it waits, then
/// types a line or presses a key.
const ROUNDS: usize = 400;

/// What [`send_keywords`] did.
#[derive(Debug, Default, PartialEq, Eq)]
struct Sent {
    /// Keywords typed at a prompt, and keywords no fresh conversation came to a prompt for.
    sent: usize,
    unsent: usize,
    /// Conversations that ended, and that stopped waiting.
    ended: usize,
    stopped: usize,
}

/// Sends `keys` a few per conversation, then the goodbye (`talk` runs one fresh conversation
/// on its lines and says whether it ended and how many lines it took at a prompt). The ones a
/// conversation did not take (it ended or stopped first) go to the next one, until each is
/// sent or a fresh conversation takes none: the rest are not sent.
fn send_keywords(keys: &[&str], mut talk: impl FnMut(&[&str]) -> (bool, usize)) -> Sent {
    let mut out = Sent::default();
    let mut pending = keys;
    while !pending.is_empty() {
        let chunk = &pending[..pending.len().min(6)];
        let mut lines = chunk.to_vec();
        lines.extend([BYE; 3]);
        let (done, took) = talk(&lines);
        out.ended += usize::from(done);
        out.stopped += usize::from(!done);
        let k = took.min(chunk.len());
        if k == 0 {
            out.unsent += pending.len();
            break;
        }
        out.sent += k;
        pending = &pending[k..];
    }
    out
}

#[test]
fn keywords_a_conversation_did_not_take_go_to_the_next_one() {
    // Synthetic: the first conversation takes three of six keywords and ends, the second gets
    // exactly the other three and the last two, then the goodbye.
    let keys = ["k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8"];
    let mut calls: Vec<Vec<String>> = Vec::new();
    let mut took = [3usize, 9].into_iter();
    let k = send_keywords(&keys, |lines| {
        calls.push(lines.iter().map(|l| l.to_string()).collect());
        (true, took.next().unwrap())
    });
    assert_eq!(
        calls,
        [
            vec!["k1", "k2", "k3", "k4", "k5", "k6", BYE, BYE, BYE],
            vec!["k4", "k5", "k6", "k7", "k8", BYE, BYE, BYE],
        ]
    );
    let want = Sent {
        sent: 8,
        unsent: 0,
        ended: 2,
        stopped: 0,
    };
    assert_eq!(k, want);
    // A conversation that takes nothing ends the retries: the rest are not sent.
    let mut took = [2usize, 0].into_iter();
    let k = send_keywords(&keys, |_| (false, took.next().unwrap()));
    let want = Sent {
        sent: 2,
        unsent: 6,
        ended: 0,
        stopped: 2,
    };
    assert_eq!(k, want);
}

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
/// after each line ends on one of them). Whether the conversation ended, and how many of
/// `lines` it took (typed at a prompt).
fn talk_through(base: &State, files: &Files, npc: i16, lines: &[&str]) -> (bool, usize) {
    let you_say = base.converse.text.as_ref().quiet().get(Msg::YouSay);
    let prompt: Vec<u8> = you_say.trim_ascii().to_vec();
    let leader = base.actors.party.leader_index;
    let mut e = Engine::with_game(base.clone(), files.clone(), move |st, io| async move {
        talk(&Cx::new(&st, &io), leader, npc, true).await;
    });
    let (mut sent, mut tries) = (0, 0);
    for _ in 0..ROUNDS {
        for _ in 0..32 {
            let before = screen(&e.state());
            e.tick(&[], 16);
            if e.finished() || (e.io().pending_events() == 0 && screen(&e.state()) == before) {
                break;
            }
        }
        if e.finished() {
            return (true, sent);
        }
        let shown = screen(&e.state()).1;
        let asked = e.state().ui.input.line_input || shown.trim_ascii_end().ends_with(&prompt);
        let keys: Vec<InputEvent> = if asked {
            let Some(line) = lines.get(sent) else {
                return (false, sent);
            };
            (sent, tries) = (sent + 1, 0);
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
    (e.finished(), sent)
}

#[test]
fn the_real_conversations_read_only_what_the_disassembler_found() {
    let Some(files) = files() else { return };
    let all = scripts(&files);
    let disasms: Vec<(u8, u16, &[u8], Disasm)> = all
        .iter()
        .map(|(l, i, b)| {
            let d = disassemble(*l, *i, b).unwrap_or_else(|r| {
                panic!("script {}{i}: {}", char::from(*l), reason_label(r.reason))
            });
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
    // the keywords, where an answer may wait on a key no generic driver gives. Every read of
    // every conversation is checked: one that does not end, as far as it went
    // (`trace::check_interrupted`).
    let talk = |npc: i16,
                lines: &[&str],
                keywords: bool,
                total: &mut Counts,
                read: &mut BTreeSet<(u8, u16, i32)>| {
        trace::start(true);
        let (done, sent) = if keywords {
            talk_through(&base, &files, npc, lines)
        } else {
            (converse(&base, &files, npc, lines).1, lines.len())
        };
        let events = trace::stop();
        assert!(
            done || keywords,
            "actor slot {npc:#x}: the conversation ends"
        );
        let n = if done {
            trace::check(&events, &decoded)
        } else {
            trace::check_interrupted(&events, &decoded)
        };
        if n.failures() > 0 {
            println!("actor slot {npc:#x}: {n:?}");
        }
        total.add(&n);
        table_reads(&events, read);
        let loaded = events.iter().find_map(|e| match e {
            Event::Load { letter, item } => Some((*letter, *item)),
            _ => None,
        });
        (loaded, done, sent)
    };
    for &npc in &npcs {
        let mut s = base.clone();
        let (name, _) = name_from_script(&mut s, &files, npc);
        if name.is_empty() {
            nameless += 1;
            continue;
        }
        let lines = ["name", "job", BYE, BYE, BYE];
        let (loaded, ..) = talk(npc, &lines, false, &mut total, &mut read);
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
    // Keywords: sent (typed at a prompt), and not sent because a fresh conversation came to no
    // prompt within `talk_through`'s rounds. Conversations: ended and not.
    let (mut keywords, mut sent, mut unsent) = (0, 0, 0);
    let (mut ended, mut stopped) = (0, 0);
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
        keywords += keys.len();
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        let k = send_keywords(&keys, |lines| {
            let (_, done, took) = talk(npc, lines, true, &mut total, &mut read);
            (done, took)
        });
        (sent, unsent) = (sent + k.sent, unsent + k.unsent);
        (ended, stopped) = (ended + k.ended, stopped + k.stopped);
    }
    println!(
        "keywords {keywords} of {} actors: sent {sent}, not sent (a fresh conversation came to no prompt within {ROUNDS} driver rounds) {unsent}; keyword conversations ended {ended}, not ended {stopped}",
        script_of.len()
    );
    assert!(
        sent + unsent == keywords,
        "every keyword sent, or no prompt for it within the driver's rounds"
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
        "{pass}: offsets checked {}, visits filled {}, table strings read {} ({distinct} distinct), table prints compared {}, table words read {}, table ends {}, table words written {}, table ops with an address outside the stack {}, mismatches {} (bytes {}, partial visits {}, fill {}, print {}, table {}, past the item {}, unknown script {})",
        n.offsets,
        n.filled,
        n.table_strings,
        n.prints,
        n.table_words,
        n.table_ends,
        n.table_writes,
        n.outside_reads,
        n.mismatches,
        n.byte_mismatches,
        n.partial_visits,
        n.fill_mismatches,
        n.print_mismatches,
        n.table_mismatches,
        n.past_item,
        n.unknown_script
    );
}

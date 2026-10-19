//! Real data (owner-local): the disassembler over every script of the owner's `converse.a` and
//! `converse.b`, and the execution cross-check on the real-data conversations of
//! `converse/tests_real.rs`, then with every actor asked the keywords of its script, each at a
//! prompt of a fresh conversation. An actor whose fresh conversation ends before any prompt is
//! sent none, and those answers are not checked (how many actors, only the output says); one
//! whose fresh conversation waits out the driver's rounds before a prompt, or that gets some of
//! its keywords sent and not the others, fails the test. Skips where the zip is missing. Its
//! output, passing or failing, holds only counts of what the test made (scripts refused by
//! reason, conversations and their ticks, actors not checked, checks, hits, misses and
//! mismatches), labels, actor slots and pass/fail (spec 01): never a value or a length of the
//! owner's data (how many scripts, items, runs, strings, keywords or bundles his files hold, or
//! what a pack of them holds), a byte, a text, a name, a keyword or a hash of them.
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
use super::{Disasm, Kind, Reason, disassemble};
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
    // Refusals by reason (the test's outcome: none for a pass); no total of the owner's scripts,
    // items, runs, strings, splits or hazards is shown.
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    let (mut decoded, mut bytes, mut once) = (0, 0usize, 0usize);
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
        assert!(
            covers_once(&d, b),
            "script {}{item}: coverage",
            char::from(*letter)
        );
        once += d.items.iter().map(|i| i.len as usize).sum::<usize>();
    }
    let catalog = Catalog::build(all.iter().map(|(l, i, b)| (*l, *i, b.as_slice())));
    for (r, n) in &refused {
        println!("refused: {r} x{n}");
    }
    assert!(refused.is_empty(), "refused: {refused:?}");
    assert!(decoded > 0 && decoded == all.len(), "every script decodes");
    println!("every script decodes: pass");
    assert!(once == bytes, "every byte in exactly one item");
    println!("every byte in exactly one item: pass");
    let back = Catalog::from_json(&catalog.to_json());
    assert!(back.is_ok_and(|c| c == catalog), "the catalog reads back");
    println!("the catalog reads back from its JSON: pass");
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
    /// Of those not sent, the ones whose last conversation ended before a prompt (the others'
    /// ran out of the driver's rounds waiting elsewhere).
    unsent_ended: usize,
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
            if done {
                out.unsent_ended += pending.len();
            }
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
    let mut took = [3usize, 8].into_iter();
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
        ended: 2,
        ..Sent::default()
    };
    assert_eq!(k, want);
    // A conversation that takes nothing ends the retries: the rest are not sent.
    let mut took = [2usize, 0].into_iter();
    let k = send_keywords(&keys, |_| (false, took.next().unwrap()));
    let want = Sent {
        sent: 2,
        unsent: 6,
        stopped: 2,
        ..Sent::default()
    };
    assert_eq!(k, want);
    // One that ends before any prompt: none sent, the conversation ended.
    let k = send_keywords(&keys, |_| (true, 0));
    let want = Sent {
        unsent: 8,
        unsent_ended: 8,
        ended: 1,
        ..Sent::default()
    };
    assert_eq!(k, want);
}

/// The keyword pass's check of one actor: all of its keywords sent or none (a fresh
/// conversation comes to its first prompt the same way whatever the lines: they are typed
/// there), and every one not sent left because its conversation ended first, not because it
/// waited out the driver's rounds (a wait no driver key ends).
fn sent_ok(k: &Sent) -> bool {
    (k.unsent == 0 || k.sent == 0) && k.unsent == k.unsent_ended
}

#[test]
fn an_actor_whose_keywords_wait_out_the_driver_fails_the_keyword_check() {
    // Synthetic: a fresh conversation that waits out the rounds before any prompt fails the
    // check; one that ends before any prompt, or takes every keyword, passes; some sent and the
    // rest not fails.
    let keys = ["k1", "k2", "k3"];
    assert!(!sent_ok(&send_keywords(&keys, |_| (false, 0))));
    assert!(sent_ok(&send_keywords(&keys, |_| (true, 0))));
    assert!(sent_ok(&send_keywords(&keys, |lines| (true, lines.len()))));
    let mut took = [2usize, 0].into_iter();
    assert!(!sent_ok(&send_keywords(&keys, |_| (
        true,
        took.next().unwrap()
    ))));
}

/// What the message scroll shows: its lines and the one being written.
fn screen(s: &State) -> (usize, Vec<u16>) {
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
    let prompt: Vec<u16> = you_say.trim_ascii().iter().map(|&b| u16::from(b)).collect();
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
        // Its cells without the ASCII white space at the end (as `trim_ascii_end`).
        let end = shown
            .iter()
            .rposition(|&c| !u8::try_from(c).is_ok_and(|b| b.is_ascii_whitespace()))
            .map_or(0, |i| i + 1);
        let asked = e.state().ui.input.line_input || shown[..end].ends_with(&prompt);
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
    let mut talked = 0;
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
            continue;
        }
        let lines = ["name", "job", BYE, BYE, BYE];
        let (loaded, ..) = talk(npc, &lines, false, &mut total, &mut read);
        script_of.push((npc, loaded.expect("the conversation loads its script")));
        talked += 1;
    }
    println!("conversations {talked}");
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
    // Keywords typed at a prompt (only whether any were: their number is the scripts'). Actors
    // whose keywords were not sent: a fresh conversation came to no prompt within
    // `talk_through`'s rounds (`sent_ok`: it ended first). Conversations: ended and not.
    let mut sent = 0;
    let (mut ended, mut stopped, mut mute) = (0, 0, 0);
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
        let k = send_keywords(&keys, |lines| {
            let (_, done, took) = talk(npc, lines, true, &mut total, &mut read);
            (done, took)
        });
        assert!(
            sent_ok(&k),
            "actor slot {npc:#x}: a keyword not sent while others were, or for a wait"
        );
        sent += k.sent;
        (ended, stopped) = (ended + k.ended, stopped + k.stopped);
        mute += usize::from(k.unsent > 0);
    }
    assert!(sent > 0, "keywords typed at a prompt");
    println!(
        "keyword conversations ended {ended}, not ended {stopped}; actors whose fresh conversation ended before a prompt (their answers are not checked) {mute}; none waited out the {ROUNDS} driver rounds: pass"
    );
    print_counts("all", &total, read.len());
    assert!(first.offsets > 0 && first.filled > 0);
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

/// Spec 09 Acceptance 5 (the bundles' receipt): the owner's files export one bundle per
/// script of both archives, in the archives' order; each reads back from its JSON; each text
/// segment's key resolves to a run of 08's catalog of that script (with its text), and every
/// run and table string of the catalog is in the bundle once (0 gaps). Prints pass/fail only:
/// no count or size of what his scripts hold.
#[test]
fn every_script_exports_a_bundle_holding_its_catalog_runs_once() {
    use super::bundle::{self, Segment};
    use super::catalog::ScriptUnits;
    let Some(files) = files() else { return };
    let all = scripts(&files);
    let out = bundle::export(&files).quiet();
    let keys: Vec<(u8, u16)> = all.iter().map(|(l, i, _)| (*l, *i)).collect();
    let got: Vec<(u8, u16)> = out.iter().map(|b| (b.archive as u8, b.item)).collect();
    assert!(got == keys, "one bundle per script, in order");
    let (mut named, mut greeting, mut turns, mut actors, mut traits) = (0, 0, 0, 0, 0);
    let (mut refused, mut gaps, mut unresolved) = (0, 0, 0);
    let (mut most, mut spent) = (0usize, 0usize);
    for (b, (l, i, bytes)) in out.iter().zip(&all) {
        let u = ScriptUnits::build(*l, *i, bytes);
        let (_, n) = bundle::Bundle::build_within(*l, *i, bytes, &[], usize::MAX);
        most = most.max(n);
        spent += n;
        let c = bundle::coverage(b, &u);
        refused += usize::from(b.refused.is_some());
        named += usize::from(b.name.as_ref().is_some_and(|n| !n.is_empty()));
        greeting += usize::from(
            b.speech
                .greeting
                .iter()
                .any(|s| matches!(s, Segment::Text(_))),
        );
        turns += usize::from(!b.turns.is_empty());
        actors += usize::from(!b.actors.is_empty());
        traits += usize::from(!b.traits.is_empty());
        gaps += c.gaps();
        unresolved += c.unresolved;
    }
    // The items spent are a size of the owner's scripts: the budgets are checked, not shown.
    assert!(
        most <= bundle::SCRIPT_BUDGET && spent <= bundle::EXPORT_BUDGET,
        "every script and the export within their budgets"
    );
    println!("every script and the export within their budgets: pass");
    assert_eq!(refused, 0, "no script refused");
    assert!(named > 0 && greeting > 0 && turns > 0 && actors > 0 && traits > 0);
    assert_eq!(
        unresolved, 0,
        "every text segment's key resolves in the catalog"
    );
    assert_eq!(
        gaps, 0,
        "every catalog run and string is in its bundle once"
    );
    println!("every text key resolves in the catalog: pass; 0 unexplained gaps: pass");
    let back = bundle::from_json(&bundle::to_json(&out));
    assert!(back.is_ok_and(|b| b == out), "the bundles read back");
    println!("the bundles read back from their JSON: pass");
}

/// What the player sees at each tick of a conversation: the hash of the frame shown and of the
/// scroll (its lines and the one being written).
type Seen = Vec<(u64, u64)>;

/// `talk_through`'s conversation with `npc`, recording what the player sees at every tick:
/// whether it ended, the hashes, and the locale layer's counts (hits, misses) after it.
fn talk_seen(base: &State, files: &Files, npc: i16, lines: &[&str]) -> (bool, Seen, [u32; 6]) {
    use super::catalog::hash64;
    let you_say = base.converse.text.as_ref().quiet().get(Msg::YouSay);
    let prompt: Vec<u16> = you_say.trim_ascii().iter().map(|&b| u16::from(b)).collect();
    let leader = base.actors.party.leader_index;
    let mut e = Engine::with_game(base.clone(), files.clone(), move |st, io| async move {
        talk(&Cx::new(&st, &io), leader, npc, true).await;
    });
    let mut seen = Vec::new();
    let mut tick = |e: &mut Engine, keys: &[InputEvent]| {
        e.tick(keys, 16);
        let s = e.state();
        let (n, last) = screen(&s);
        let text: Vec<u8> =
            s.ui.messages
                .iter()
                .flat_map(|l| l.bytes().chain([b'\n']))
                .chain(n.to_le_bytes())
                .chain(last.iter().flat_map(|c| c.to_le_bytes()))
                .collect();
        seen.push((hash64(e.frame().pixels()), hash64(&text)));
    };
    let counts = |e: &Engine| {
        let l = &e.state().locale;
        let ([th, tm], [xh, xm]) = (l.tables.get(), l.texts.get());
        [l.hits, l.misses, th, tm, xh, xm]
    };
    let (mut sent, mut tries) = (0, 0);
    for _ in 0..ROUNDS {
        for _ in 0..32 {
            let before = screen(&e.state());
            tick(&mut e, &[]);
            if e.finished() || (e.io().pending_events() == 0 && screen(&e.state()) == before) {
                break;
            }
        }
        if e.finished() {
            let c = counts(&e);
            return (true, seen, c);
        }
        let shown = screen(&e.state()).1;
        let end = shown
            .iter()
            .rposition(|&c| !u8::try_from(c).is_ok_and(|b| b.is_ascii_whitespace()))
            .map_or(0, |i| i + 1);
        let asked = e.state().ui.input.line_input || shown[..end].ends_with(&prompt);
        let keys: Vec<InputEvent> = if asked {
            let Some(line) = lines.get(sent) else {
                let c = counts(&e);
                return (false, seen, c);
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
        tick(&mut e, &keys);
    }
    let c = counts(&e);
    (e.finished(), seen, c)
}

/// Spec 08 Order 3 / Acceptance 5, for the conversation (slices K1 and K2): the identity pack
/// built at test time from the owner's files (every run and every table string of the catalog,
/// and every executable string the engine catalogues, each translated as itself, declaring
/// English grammar; `crate::locale::identity`), written as a pack file and read back through
/// the pack reader, checked against the files and installed (`wide` on): the conversations of
/// the cross-check (name, job and the goodbye with the 40 actors nearest the leader, and the
/// greeter) show at every tick the frame and the scroll they show without it, and every run,
/// table string and catalogued executable string they show is one of the pack's (0 misses).
/// Prints the counts of its own conversations (ticks, hits, misses) and pass/fail only, no size
/// of the pack.
#[test]
fn the_identity_pack_shows_the_real_conversations_exactly_as_english() {
    use crate::locale;
    let Some(files) = files() else { return };
    let made = locale::identity(&files).quiet();
    let base = with_text(&files);
    let mut with = base.clone();
    locale::install(&mut with, &files, made.clone(), None).quiet();
    assert!(with.ui.text.wide, "wide on");
    // The actors of the cross-check.
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
    let mut s = base.clone();
    let greeter = (2..0xe0i16).find(|&npc| {
        let mut buf = Vec::new();
        load_script(&mut s, &files, npc, ScriptDst::Other(&mut buf)) != 0
            && buf.windows(2).any(|w| w == b"$G")
            && buf.windows(2).any(|w| w == b"$T")
    });
    npcs.extend(greeter);
    let (mut talked, mut ticks, mut differ, mut ended) = (0, 0, 0, 0);
    let mut counts = [0u32; 6];
    for &npc in &npcs {
        let mut s = base.clone();
        if name_from_script(&mut s, &files, npc).0.is_empty() {
            continue;
        }
        let lines = ["name", "job", BYE, BYE, BYE];
        let (done_en, en, _) = talk_seen(&base, &files, npc, &lines);
        let (done_id, id, c) = talk_seen(&with, &files, npc, &lines);
        talked += 1;
        ticks += en.len();
        ended += usize::from(done_en && done_id);
        if id != en || done_id != done_en {
            differ += 1;
            println!("a conversation differs with the identity pack");
        }
        for (sum, n) in counts.iter_mut().zip(c) {
            *sum += n;
        }
    }
    let [
        hits,
        misses,
        table_hits,
        table_misses,
        text_hits,
        text_misses,
    ] = counts;
    println!(
        "identity pack: conversations {talked} (ended {ended}), ticks compared {ticks}, differing conversations {differ}; from the pack: runs {hits}, table strings {table_hits}, game strings {text_hits}; misses: runs {misses}, table strings {table_misses}, game strings {text_misses}"
    );
    assert!(talked > 0 && hits > 0 && text_hits > 0);
    assert_eq!(differ, 0, "every frame and scroll as in English");
    assert_eq!(
        [misses, table_misses, text_misses],
        [0, 0, 0],
        "every unit shown is in the pack"
    );
    println!("identity pack frames and scroll identical to English: pass; misses 0: pass");
}

/// Spec 08 (slices K1 and K2): the pack the translation tool makes from the owner's NPC bundles
/// checks against his files. Keyed as `tools/i18n/translate.mjs` keys it (each text segment's
/// key, its text one byte per character for the length and the hash, the fingerprint over each
/// script's bundle `hash`; each table string's key and text the same way, with its `capacity`,
/// an empty one or one an op reads from inside left out, `tablesOf`), every run and table string
/// of every bundle that has one goes into one pack of English grammar (each translated as
/// itself; a capacity its own English passes is not written, as the tool's gate would refuse
/// it); written as a file, read back and installed on the files the bundles came from. Prints
/// pass/fail only, no size of the pack.
#[test]
fn a_pack_keyed_from_the_owners_bundles_checks_against_his_files() {
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use super::bundle::{self, Segment};
    use crate::locale::{self, Pack, RunText, TableText, fingerprint};
    use crate::ui::Language;
    let Some(files) = files() else { return };
    let bundles = bundle::export(&files).quiet();
    let mut runs: BTreeMap<(u8, u16, u32), RunText> = BTreeMap::new();
    let mut tables: BTreeMap<(u8, u16, u32), TableText> = BTreeMap::new();
    let mut named = Vec::new();
    for b in &bundles {
        let letter = b.archive as u8;
        let before = runs.len() + tables.len();
        for t in b
            .strings
            .iter()
            .filter(|t| !t.text.is_empty() && t.inner.is_empty())
        {
            let bytes: Vec<u8> = t.text.chars().map(|c| c as u8).collect();
            tables.insert(
                (letter, b.item, t.key.2),
                TableText {
                    len: bytes.len() as u32,
                    hash: super::catalog::hash64(&bytes),
                    capacity: t.capacity.filter(|&c| bytes.len() <= c as usize),
                    text: bytes,
                },
            );
        }
        for s in b.segments() {
            let Segment::Text(t) = s else { continue };
            let bytes: Vec<u8> = t.text.chars().map(|c| c as u8).collect();
            runs.insert(
                (letter, b.item, t.key.2),
                RunText {
                    len: bytes.len() as u32,
                    hash: super::catalog::hash64(&bytes),
                    text: bytes,
                },
            );
        }
        if runs.len() + tables.len() > before {
            named.push((letter, b.item, b.hash));
        }
    }
    let pack = Pack::new(Language::English, "bundles/1", fingerprint(named), runs)
        .quiet()
        .with_tables(tables)
        .quiet();
    let read = Pack::parse(&pack.to_bytes()).quiet();
    let mut s = with_text(&files);
    let checked = locale::install(&mut s, &files, Rc::new(read), None);
    println!(
        "pack from the bundles checks against the files: {}",
        if checked.is_ok() { "pass" } else { "fail" }
    );
    assert!(pack.runs() > 0 && pack.tables() > 0);
    assert!(checked.is_ok(), "the pack keyed from the bundles checks");
}

/// Spec 08 (slice K1): the translation tool's keyword rule (`tools/i18n/translate.mjs`
/// `keywordsOf`: an `@` keyword is the ASCII letters and digits right after the `@`) gives, for
/// every `@` of every run of the owner's scripts, the keyword the escape layer shows: from the
/// `@` up to a character of the game's keyword-end list (`ui.tables.keyword_end`), the next
/// escape prefix or the run's end, the characters the escape layer takes before the prefix (`&`,
/// `<`, `>`, the back-quote, `*`) putting nothing. Prints pass/fail only: how many `@` the
/// runs hold is a count of his text.
#[test]
fn the_tools_keyword_rule_is_the_escape_layers_on_the_owners_runs() {
    use super::Escape;
    let Some(files) = files() else { return };
    let ends = with_text(&files).ui.tables.keyword_end.clone();
    let all = scripts(&files);
    let catalog = Catalog::build(all.iter().map(|(l, i, b)| (*l, *i, b.as_slice())));
    let (mut keywords, mut agree) = (0, 0);
    for s in &catalog.scripts {
        for r in &s.runs {
            let text: Vec<u8> = r.unit.text.chars().map(|c| c as u8).collect();
            for (at, &b) in text.iter().enumerate() {
                if b != b'@' {
                    continue;
                }
                keywords += 1;
                let rest = &text[at + 1..];
                let mut engine = Vec::new();
                for &c in rest {
                    if matches!(c, b'&' | b'<' | b'>' | b'`' | b'*') {
                        continue;
                    }
                    if c == 0 || ends.contains(&c) || Escape::start(c).is_some() {
                        break;
                    }
                    engine.push(c.to_ascii_lowercase());
                }
                let tool: Vec<u8> = rest
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .map(u8::to_ascii_lowercase)
                    .collect();
                agree += usize::from(engine == tool);
            }
        }
    }
    assert!(keywords > 0);
    assert!(
        agree == keywords,
        "the tool's keyword rule is the escape layer's"
    );
    println!("the tool's keyword rule matches the escape layer on every @: pass");
}

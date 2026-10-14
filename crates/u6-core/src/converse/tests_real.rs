//! Real data: conversations with NPCs near the party start, from the user's archive.org zip.
//! Skips where the zip is missing; prints actor numbers, line counts and lengths, never the
//! dialogue or a name (no game text in the repo or the test output).
#![allow(clippy::disallowed_methods)] // tests may read files

use std::path::Path;

use u6_formats::converse::ConverseLib;
use u6_intake::{InputFile, intake};
use u6_testkit::assert_same;

use super::party::name_from_script;
use super::script::{ScriptDst, load_script};
use super::text::ConverseText;
use super::{Cx, Msg, talk};
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::objects::ObjectId;
use crate::state::State;
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

fn with_text(files: &Files) -> State {
    let mut s = State::new_game(files).quiet();
    // Load state's tail: the region's objects and the view the on-screen test reads; main's
    // text windows (the message scroll current).
    crate::main_loop::load_state_tail(&mut s);
    let t = s.ui.tables.clone();
    s.ui.text.main_windows(&t);
    s.ui.text.more_rows = i16::from(s.ui.text.win().height);
    let text = ConverseText::from_game(files.get("game").quiet()).quiet();
    s.converse.text = Some(text);
    s
}

fn line_keys(line: &str) -> Vec<InputEvent> {
    let mut v: Vec<InputEvent> = line
        .chars()
        .map(|c| InputEvent::Key(Key::Char(c)))
        .collect();
    v.push(InputEvent::Key(Key::Enter));
    v
}

/// The scroll lines so far, the one being written last.
fn lines_of(s: &State) -> Vec<String> {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let mut last: String = t.transcript_line.iter().map(|&b| char::from(b)).collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| char::from(b)));
    lines.push(last);
    lines
}

/// Ticks without input until the game waits for a key with nothing queued (or ends).
fn settle(e: &mut Engine) {
    for _ in 0..32 {
        let before = lines_of(&e.state());
        e.tick(&[], 16);
        if e.finished() || (e.io().pending_events() == 0 && lines_of(&e.state()) == before) {
            return;
        }
    }
}

/// Runs Talk (0x1127c4) on `npc`. At each prompt for the player's words (`Msg::YouSay`) it
/// types the next of `lines`; at any other wait (a `*` page break, a key wait, a one-key
/// question) it presses Space, then N, then Y until the game moves on. Returns the scroll and
/// whether the conversation ended.
fn converse(state: &State, files: &Files, npc: i16, lines: &[&str]) -> (Vec<String>, bool) {
    let leader = state.actors.party.leader_index;
    let prompt = latin(state.converse.text.as_ref().quiet().get(super::Msg::YouSay));
    let prompt = prompt.trim().to_string();
    let mut e = Engine::with_game(state.clone(), files.clone(), move |st, io| async move {
        talk(&Cx::new(&st, &io), leader, npc, true).await;
    });
    let mut lines = lines.iter();
    let mut tries = 0;
    let mut seen = Vec::new();
    for _ in 0..200 {
        settle(&mut e);
        if e.finished() {
            break;
        }
        let now = lines_of(&e.state());
        if now != seen {
            tries = 0;
            seen = now;
        }
        let last = seen.last().cloned().unwrap_or_default();
        if e.state().ui.input.line_input || last.trim_end().ends_with(&prompt) {
            let Some(line) = lines.next() else { break };
            e.tick(&line_keys(line), 16);
            tries = 0;
        } else {
            let k = [' ', 'n', 'y'][tries % 3];
            e.tick(&[InputEvent::Key(Key::Char(k))], 16);
            tries += 1;
        }
    }
    let done = e.finished();
    (lines_of(&e.state()), done)
}

#[test]
fn every_script_loads_like_the_formats_parser() {
    let Some(files) = files() else { return };
    let mut s = with_text(&files);
    let lib_a = ConverseLib::parse(files.get("converse.a").quiet()).quiet();
    let lib_b = ConverseLib::parse(files.get("converse.b").quiet()).quiet();
    let mut loaded = 0;
    for npc in 0..0xe0i16 {
        let mut buf = Vec::new();
        let got = load_script(&mut s, &files, npc, ScriptDst::Other(&mut buf));
        let (lib, item) = if npc > 0x62 {
            (&lib_b, npc - 0x63)
        } else {
            (&lib_a, npc)
        };
        let want = lib.scripts.get(item as usize).cloned().flatten();
        match want {
            Some(script) if npc != 0 => {
                assert!(got == npc);
                assert!(script.bytecode.len() < super::SCRIPT_LEN, "npc {npc}: size");
                assert_same!(
                    buf[..script.bytecode.len()],
                    script.bytecode[..],
                    "the script of npc {npc}"
                );
                assert!(buf[0] == 0xff, "npc {npc}: script starts with ff");
                loaded += 1;
            }
            _ => assert!(got == 0, "npc {npc}"),
        }
    }
    assert!(loaded > 0, "scripts loaded");
}

#[test]
fn three_npcs_near_the_start_answer_name_job_bye() {
    let Some(files) = files() else { return };
    let base = with_text(&files);
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
    let prompt = latin(base.converse.text.as_ref().quiet().get(super::Msg::YouSay));
    let prompt = prompt.trim().to_string();
    let mut answered = 0;
    for &(_, npc) in near.iter().take(40) {
        let mut s = base.clone();
        let (name, _) = name_from_script(&mut s, &files, npc);
        if name.is_empty() {
            continue;
        }
        let (scroll, done) = converse(&base, &files, npc, &["name", "job", "bye"]);
        // The replies that follow each typed line, up to the next prompt.
        let replies: Vec<String> = scroll
            .split(|l| l.contains(&prompt))
            .map(|chunk| chunk.join("\n").trim().to_string())
            .collect();
        let asked = scroll.iter().filter(|l| l.contains(&prompt)).count();
        let ok = done && asked >= 3 && replies.len() >= 3 && replies[1] != replies[2];
        assert!(!has_codes(&scroll), "actor {npc}: unsubstituted variable");
        if ok && answered < 3 {
            println!("actor slot {npc:#x} answered name and job and said goodbye");
            answered += 1;
        }
    }
    assert!(
        answered == 3,
        "three NPCs answered name and job differently and said goodbye"
    );
}

/// `$x` / `#x` variable codes left in the scroll.
fn has_codes(scroll: &[String]) -> bool {
    scroll.iter().any(|l| {
        l.as_bytes()
            .windows(2)
            .any(|w| (w[0] == b'$' || w[0] == b'#') && w[1].is_ascii_alphanumeric())
    })
}

#[test]
fn greeting_substitutes_title_time_and_names() {
    let Some(files) = files() else { return };
    let base = with_text(&files);
    let text = base.converse.text.clone().quiet();
    let title = text.get(if super::seams::female(&base) {
        Msg::Milady
    } else {
        Msg::Milord
    });
    let time = text.get(match base.actors.calendar.hour {
        0..12 => Msg::Morning,
        12..18 => Msg::Afternoon,
        _ => Msg::Evening,
    });
    let (title, time) = (latin(title), latin(time));
    // The first actor (by number) whose script uses both `$G` and `$T`.
    let mut s = base.clone();
    let npc = (2..0xe0i16)
        .find(|&npc| {
            let mut buf = Vec::new();
            load_script(&mut s, &files, npc, ScriptDst::Other(&mut buf)) != 0
                && buf.windows(2).any(|w| w == b"$G")
                && buf.windows(2).any(|w| w == b"$T")
        })
        .quiet();
    let (scroll, done) = converse(&base, &files, npc, &["name", "job", "bye"]);
    let all = scroll.join("\n");
    assert!(done && !has_codes(&scroll));
    assert!(
        all.contains(&title) && all.contains(&time),
        "substituted title and time of day"
    );
}

fn latin(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

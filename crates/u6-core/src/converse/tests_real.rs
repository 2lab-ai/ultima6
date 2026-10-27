//! Real data: conversations with NPCs near the party start, from the user's archive.org zip.
//! Skips where the zip is missing; prints actor numbers and pass/fail, never the dialogue, a
//! name, or how many lines or how long (no game text in the repo or the test output).
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

pub(super) fn files() -> Option<Files> {
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

pub(super) fn with_text(files: &Files) -> State {
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

/// Ticks without input until the game waits for a key with nothing queued (or ends); `watch`
/// sees the engine after each tick.
fn settle(e: &mut Engine, watch: &mut impl FnMut(&Engine)) {
    for _ in 0..32 {
        let before = lines_of(&e.state());
        e.tick(&[], 16);
        watch(e);
        if e.finished() || (e.io().pending_events() == 0 && lines_of(&e.state()) == before) {
            return;
        }
    }
}

/// Runs Talk (0x1127c4) on `npc`. At each prompt for the player's words (`Msg::YouSay`) it
/// types the next of `lines`; at any other wait (a `*` page break, a key wait, a one-key
/// question) it presses Space, then N, then Y until the game moves on. Returns the scroll and
/// whether the conversation ended.
pub(super) fn converse(
    state: &State,
    files: &Files,
    npc: i16,
    lines: &[&str],
) -> (Vec<String>, bool) {
    let e = converse_watched(state, files, npc, lines, false, |_| {});
    let done = e.finished();
    (lines_of(&e.state()), done)
}

/// [`converse`]'s run, the engine kept: with `paper` the boot's paper drawn first
/// ([`crate::main_loop::draw_paper`]); `watch` sees the engine after each tick.
fn converse_watched(
    state: &State,
    files: &Files,
    npc: i16,
    lines: &[&str],
    paper: bool,
    mut watch: impl FnMut(&Engine),
) -> Engine {
    let leader = state.actors.party.leader_index;
    let prompt = latin(state.converse.text.as_ref().quiet().get(super::Msg::YouSay));
    let prompt = prompt.trim().to_string();
    let mut e = Engine::with_game(state.clone(), files.clone(), move |st, io| async move {
        if paper {
            crate::main_loop::draw_paper(&mut st.borrow_mut(), &io);
        }
        talk(&Cx::new(&st, &io), leader, npc, true).await;
    });
    let mut lines = lines.iter();
    let mut tries = 0;
    let mut seen = Vec::new();
    for _ in 0..200 {
        settle(&mut e, &mut watch);
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
            watch(&e);
            tries = 0;
        } else {
            let k = [' ', 'n', 'y'][tries % 3];
            e.tick(&[InputEvent::Key(Key::Char(k))], 16);
            watch(&e);
            tries += 1;
        }
    }
    e
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

/// The actors on the leader's level, nearest first (party members and empty slots aside).
fn near_the_start(base: &State) -> Vec<i16> {
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
    near.into_iter().map(|(_, a)| a).collect()
}

#[test]
fn three_npcs_near_the_start_answer_name_job_bye() {
    let Some(files) = files() else { return };
    let base = with_text(&files);
    let near = near_the_start(&base);
    let prompt = latin(base.converse.text.as_ref().quiet().get(super::Msg::YouSay));
    let prompt = prompt.trim().to_string();
    let mut answered = 0;
    for &npc in near.iter().take(40) {
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

/// The message window's rows of `fb` (spec 23: cells 22..=38 × 14..=23), each its 8 pixel rows
/// of colours.
fn message_rows(fb: &u6_render::Framebuffer, m: &crate::ui::text::TextWindow) -> Vec<Vec<u8>> {
    let (x0, w) = (usize::from(m.x) * 8, usize::from(m.width) * 8);
    (0..usize::from(m.height))
        .map(|r| {
            let y0 = (usize::from(m.y) + r) * 8;
            (0..8 * w)
                .map(|k| fb.get(x0 + k % w, y0 + k / w).quiet())
                .collect()
        })
        .collect()
}

/// Spec 23's real-data receipt: conversations near the start on the boot's paper, long enough to
/// scroll the message window, then the wheel back a line at a time. Each line the window scrolled
/// off shows, in the view's top row, the colours its row showed on the published frames before it
/// scrolled off (rows above the cursor, which nothing draws again), the scrollbar's pixels (its
/// cell column's last three pixels in the ink, over a cell of one colour) aside. Counts only.
#[test]
fn receipt_a_conversation_scrolled_back_shows_each_line_as_it_was() {
    use crate::ui::text::WindowId;
    use std::collections::HashMap;
    let Some(files) = files() else { return };
    let mut base = with_text(&files);
    base.main.program = crate::main_loop::Program::Game;
    let m = *base.ui.text.window(WindowId::Messages);
    let ink = base.ui.tables.colours.text;
    let w = usize::from(m.width) * 8;
    let bar = (w - 3)..w;
    let (mut talks, mut kept, mut compared, mut equal, mut barred) = (0, 0, 0, 0, 0);
    let (mut spans, mut patched, mut agree) = (0, 0, 0);
    for &npc in near_the_start(&base).iter().take(40) {
        let mut s = base.clone();
        if name_from_script(&mut s, &files, npc).0.is_empty() {
            continue;
        }
        // Every published frame: the lines kept so far, the cursor row, the window's rows.
        let mut shots: Vec<(u64, u8, Vec<Vec<u8>>)> = Vec::new();
        let mut version = 0;
        let lines = ["name", "job", "bye"];
        let mut e = converse_watched(&base, &files, npc, &lines, true, |e| {
            if e.published_version() == version {
                return;
            }
            version = e.published_version();
            let st = e.state();
            let row = st.ui.text.window(WindowId::Messages).row;
            let rows = message_rows(&e.published_frame(), &m);
            shots.push((st.ui.scrollback.total(), row, rows));
        });
        // Line number -> the colours its row showed (the same from every frame it was on).
        let mut was: HashMap<u64, Vec<u8>> = HashMap::new();
        for (total, row, rows) in &shots {
            for (j, r) in rows.iter().enumerate().take(usize::from(*row)) {
                let line = total + j as u64;
                match was.get(&line) {
                    Some(prev) => {
                        assert!(
                            prev == r,
                            "a row above the cursor changed before it scrolled"
                        );
                        agree += 1;
                    }
                    None => {
                        was.insert(line, r.clone());
                    }
                }
            }
        }
        let (total, base_line, n_spans, n_patched) = {
            let sb = &e.state().ui.scrollback;
            let len = sb.lines().len() as u64;
            let spans = sb.lines().iter().filter(|l| !l.spans.is_empty()).count();
            let patched = sb.lines().iter().filter(|l| !l.patch.is_empty()).count();
            (sb.total(), sb.total() - len, spans, patched)
        };
        if total < u64::from(m.height) {
            continue;
        }
        talks += 1;
        spans += n_spans;
        patched += n_patched;
        // Back a line at a time to the oldest line kept.
        let mut back = 0;
        while e.wheel(200, 150, -1) && e.history_offset() > back {
            back = e.history_offset();
        }
        assert!(back > 0, "the wheel scrolled back");
        kept += back;
        // `U6_SCROLLBACK_FRAMES` (a directory): the first conversation's frame at the oldest line
        // and half way back, RGBA8 at the frame's size (spec 23's frames, kept outside the repo).
        let frames = std::env::var("U6_SCROLLBACK_FRAMES")
            .ok()
            .filter(|_| talks == 1);
        while e.history_offset() > 0 {
            let at = e.history_offset();
            if let Some(dir) = &frames
                && (at == back || at == back.div_ceil(2))
            {
                let mut rgba = Vec::new();
                e.frame().write_rgba(&mut rgba);
                let (w, h) = (u6_render::WIDTH, u6_render::HEIGHT);
                let name = format!(
                    "receipt-{}-back{at}-{w}x{h}.rgba",
                    if w == 320 { "original" } else { "wide" }
                );
                std::fs::write(Path::new(dir).join(name), rgba).quiet();
            }
            let line = base_line + (back - at) as u64;
            let top = e.with_view_frame(|fb| message_rows(fb, &m).swap_remove(0));
            if let Some(then) = was.get(&line) {
                compared += 1;
                // The bar draws over the last cell only where it shows one colour.
                let plain = (0..8).all(|y| (w - 8..w).all(|x| then[y * w + x] == then[w - 8]));
                let mut on_bar = false;
                let same = (0..8 * w).all(|k| {
                    if top[k] == then[k] {
                        return true;
                    }
                    let ok = plain && bar.contains(&(k % w)) && top[k] == ink;
                    on_bar |= ok;
                    ok
                });
                assert!(same, "a kept line shows the colours its row showed");
                equal += 1;
                barred += usize::from(on_bar);
            }
            assert!(e.wheel(200, 150, 1), "the wheel goes forward again");
            assert!(e.history_offset() < at, "a line forward");
        }
    }
    println!(
        "scrollback receipt: {talks} conversations scrolled; {kept} lines kept and wheeled to; \
         {compared} compared with their published rows, {equal} equal ({barred} under the bar, \
         {agree} rows seen on two frames agreed); {spans} with keyword spans, {patched} with \
         patched pixels"
    );
    assert!(talks >= 3, "three conversations scrolled the window");
    assert!(
        compared > 0 && equal == compared,
        "every compared line is as it was"
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

/// The lines `now` (the scroll's transcript, which keeps only its last lines) adds to `full`
/// (every finished line so far): those after the longest end of `full` that `now` starts with.
fn extend_transcript(full: &mut Vec<String>, now: &[String]) {
    let most = full.len().min(now.len());
    let kept = (0..=most)
        .rev()
        .find(|&k| full[full.len() - k..] == now[..k])
        .unwrap_or(0);
    full.extend_from_slice(&now[kept..]);
}

/// [`converse`] with voices on: every line the scroll showed (not only the transcript's last
/// ones) and the utterances the speech tap queued.
fn converse_said(
    state: &State,
    files: &Files,
    npc: i16,
    lines: &[&str],
) -> (Vec<String>, Vec<crate::speech::Utterance>) {
    let leader = state.actors.party.leader_index;
    let prompt = latin(state.converse.text.as_ref().quiet().get(super::Msg::YouSay));
    let prompt = prompt.trim().to_string();
    let mut s = state.clone();
    crate::speech::set_enabled(&mut s, true);
    let mut e = Engine::with_game(s, files.clone(), move |st, io| async move {
        talk(&Cx::new(&st, &io), leader, npc, true).await;
    });
    let mut full = Vec::new();
    let mut said = Vec::new();
    let mut tick = |e: &mut Engine, keys: &[InputEvent]| {
        e.tick(keys, 16);
        extend_transcript(&mut full, &e.state().ui.messages);
        said.extend(e.take_speech());
    };
    let mut lines = lines.iter();
    let mut tries = 0;
    let mut seen = Vec::new();
    for _ in 0..200 {
        for _ in 0..32 {
            let before = lines_of(&e.state());
            tick(&mut e, &[]);
            if e.finished() || (e.io().pending_events() == 0 && lines_of(&e.state()) == before) {
                break;
            }
        }
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
            tick(&mut e, &line_keys(line));
            tries = 0;
        } else {
            let k = [' ', 'n', 'y'][tries % 3];
            tick(&mut e, &[InputEvent::Key(Key::Char(k))]);
            tries += 1;
        }
    }
    full.extend(lines_of(&e.state()).last().cloned());
    (full, said)
}

#[test]
fn the_transcript_helper_appends_only_the_new_lines() {
    let mut full = Vec::new();
    let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    extend_transcript(&mut full, &v(&["a", "b"]));
    extend_transcript(&mut full, &v(&["a", "b", "c"]));
    extend_transcript(&mut full, &v(&["c", "d"]));
    extend_transcript(&mut full, &v(&["c", "d"]));
    assert_eq!(full, v(&["a", "b", "c", "d"]));
}

/// Spec 09 Acceptance 2's real-data receipt: talking to the NPCs near the start with voices on,
/// every utterance the tap queued is text the scroll shows (whitespace aside), none holds the
/// NPC's look text, the turns never go back and the sequence has no gap. Prints counts and
/// pass/fail only.
#[test]
fn speech_tap_on_npcs_near_the_start_says_what_the_scroll_shows() {
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
    let squeeze = |t: &str| t.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let (mut talked, mut utterances, mut shown, mut look_said) = (0, 0, 0, 0);
    for &(_, npc) in near.iter().take(40) {
        let mut s = base.clone();
        let mut script = Vec::new();
        if load_script(&mut s, &files, npc, ScriptDst::Other(&mut script)) == 0 {
            continue;
        }
        let look: Vec<u8> = script
            .iter()
            .skip_while(|&&b| b != 0xf1)
            .skip(1)
            .take_while(|&&b| b != 0xf2)
            .copied()
            .collect();
        let look = squeeze(&latin(&look));
        let (scroll, said) = converse_said(&base, &files, npc, &["name", "job", "bye"]);
        if said.is_empty() {
            continue;
        }
        talked += 1;
        let screen = squeeze(&scroll.join("\n"));
        for (i, u) in said.iter().enumerate() {
            utterances += 1;
            let text = squeeze(&u.text);
            if screen.contains(&text) {
                shown += 1;
            }
            if look.len() > 8 && text.contains(&look) {
                look_said += 1;
            }
            assert!(
                u.seq as usize == i,
                "actor {npc}: utterance {i} in sequence"
            );
            assert!(
                i == 0 || u.turn >= said[i - 1].turn,
                "actor {npc}: turn order"
            );
            assert!(u.speaker >= 0, "actor {npc}: a speaker");
        }
    }
    let ok = talked >= 3 && shown == utterances && look_said == 0;
    println!(
        "speech tap: at least 3 of the 40 nearest NPCs said something, all of it shown on the \
         scroll and none of it the look text: {}",
        if ok { "pass" } else { "fail" }
    );
    assert!(
        ok,
        "every utterance is on the scroll and none is the look text"
    );
}

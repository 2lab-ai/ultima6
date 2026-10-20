//! The speech tap (spec 09 Design 1, Acceptance 2) on hand-built conversations: the exact text
//! each flush queues, its speaker, its turn and the order, for the greeting, answers, the look
//! text, the player's echo, an engine message, both printing forms of `0xb5`, `0xbf`, the waits
//! that flush, and voices off; with a Korean pack (spec 09 Order 6), the language of each
//! utterance. All text here is the test's own.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::begin;
use super::disasm::catalog::ScriptUnits;
use super::seams;
use super::tests::{chars, files, lib, packed, run, scroll, state};
use super::tests_locale;
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::locale::{self, Pack};
use crate::speech::{self, Kind, Lang, Utterance};
use crate::state::State;
use crate::ui::{Language, WindowId};

const NONE: Vec<InputEvent> = Vec::new();
/// The script header of NPC 3 ("Ann", whose look text is "a woman"); its body follows.
const HEADER: &[u8] = b"\xff\x03Ann\xf1a woman\xf2";
/// Where the table of the `0xb5` table test lies in the script.
const TABLE_AT: usize = 0x200;

/// A synthetic `game` holding only the line the portrait panel prints the name with (`%s`
/// newline at 0x1283de), made up for the test.
fn name_line_game() -> Vec<u8> {
    let at = (0x0012_83de - crate::image::IMAGE_BASE) as usize;
    let mut code = vec![0u8; at + 4];
    code[at..at + 3].copy_from_slice(b"%s\n");
    crate::image::SyntheticGame::new()
        .loaded(false, code)
        .build()
}

/// NPC 3's script (`HEADER` + `body`), NPC 5 named "Zed", and the name line.
fn talk_files(body: &[u8]) -> Files {
    let mut script = HEADER.to_vec();
    script.extend_from_slice(body);
    let mut a: Vec<Option<Vec<u8>>> = vec![None; 6];
    a[3] = Some(packed(&script));
    a[5] = Some(packed(b"\xff\x05Zed\xf1"));
    files(&[("talkfile.a", lib(&a)), ("game", name_line_game())])
}

/// A conversation with NPC 3 (`HEADER` + `body`) typing `keys`, voices on or off; the
/// utterances queued and the state after it.
fn talk_in(mut s: State, body: &[u8], keys: &str, voices: bool) -> (Vec<Utterance>, State) {
    speech::set_enabled(&mut s, voices);
    let (_, mut s) = run!(s, talk_files(body), chars(keys), |cx| begin(cx, 0, 3).await);
    (speech::take(&mut s), s)
}

fn talk(body: &[u8], keys: &str) -> (Vec<Utterance>, State) {
    talk_in(state(&[]), body, keys, true)
}

/// (seq, turn, speaker, text) of each utterance.
fn said(u: &[Utterance]) -> Vec<(u32, u32, i16, &str)> {
    u.iter()
        .map(|u| (u.seq, u.turn, u.speaker, u.text.as_str()))
        .collect()
}

#[test]
fn a_greeting_an_answer_and_a_nested_answer_are_said_without_the_look_text_or_the_echo() {
    // The greeting (with `$P`), then at the first prompt "name": the text after the input op
    // and, in it, the keyword block's answer (a nested run), the engine's newline between them
    // not kept. "peek" replays the look text (not kept); "farewell" gets the last answer.
    let mut body = b"Hi $P.\xf7Plain \xefname\xf6and nested.\xee".to_vec();
    body.extend_from_slice(b"\xf7\xeffare\xf6Bye.\xb6\xee");
    let (u, s) = talk(&body, "name\npeek\nfarewell\n");
    assert_eq!(
        said(&u),
        [
            (0, 0, 3, "Hi Hero."),
            (1, 1, 3, "Plain and nested."),
            (2, 3, 3, "Bye.")
        ]
    );
    for u in &u {
        assert_eq!(u.script, Some((b'a', 3)));
        assert_eq!(u.lang, Lang::En);
    }
    // The screen has the look text twice and the echo of every input; none of it was said.
    let screen = scroll(&s);
    assert_eq!(screen.matches("a woman").count(), 2, "{screen}");
    assert!(screen.contains("say:name") && screen.contains("say:peek"));
    assert!(u.iter().all(|u| !u.text.contains("woman")
        && !u.text.contains("name")
        && !u.text.contains("peek")
        && !u.text.contains("farewell")));
}

#[test]
fn an_engine_message_inside_an_answer_is_not_said_and_both_printing_forms_of_b5_are() {
    // Op 0xd1 is unknown: the engine prints its message between "ab" and "cd". Then 0xb5's
    // table form (string 1 of the table at TABLE_AT) and its slot form (`$P`).
    let mut body = b"\xf7ab\xd1cd \xb5\xd2".to_vec();
    body.extend_from_slice(&(TABLE_AT as u32).to_le_bytes());
    body.extend_from_slice(b"\x01\xa7 \xb5\xd3\x19\xb3\xa7.\xb6");
    body.resize(TABLE_AT - HEADER.len(), 0);
    body.extend_from_slice(b"aa\0bb\0");
    let (u, s) = talk(&body, "x\n");
    assert_eq!(said(&u), [(0, 1, 3, "abcd bb Hero.")]);
    assert!(scroll(&s).contains("ab[UnknownCommand]cd bb Hero."));
}

#[test]
fn b_f_makes_its_actor_the_speaker_until_the_next_input_and_its_name_is_not_said() {
    let body = b"Hi.\xf7Before.\xbf\x05\xa7After.\xf7Back.\xb6";
    let mut s = state(&[]);
    s.actors.attrs.talk_flags[5] = 1;
    let (u, s) = talk_in(s, body, "x\ny\n", true);
    assert_eq!(
        said(&u),
        [
            (0, 0, 3, "Hi."),
            (1, 1, 3, "Before."),
            (2, 1, 5, "After."),
            (3, 2, 3, "Back.")
        ]
    );
    // The panel printed the name it read from actor 5's script (into the status window).
    assert_eq!(s.converse.name_buf, b"Zed");
    assert!(u.iter().all(|u| !u.text.contains("Zed")));
    // The script is the conversation's, whoever speaks.
    assert!(u.iter().all(|u| u.script == Some((b'a', 3))));
}

#[test]
fn a_cb_key_wait_and_a_star_wait_inside_the_words_flush_and_keep_the_turn() {
    // Op 0xcb inside the answer and a `*` wait: each flushes; a key there is no input of a turn.
    let (u, _) = talk(b"\xf7One.\xcbTwo*Three.\xb6", "x\nkk");
    assert_eq!(
        said(&u),
        [(0, 1, 3, "One."), (1, 1, 3, "Two"), (2, 1, 3, "Three.")]
    );
    // In the greeting too.
    let (u, _) = talk(b"Hi.\xcbAfter.\xb6", "k");
    assert_eq!(said(&u), [(0, 0, 3, "Hi."), (1, 0, 3, "After.")]);
}

/// What the message scroll has drawn: its finished rows, then the drawn part of the row being
/// written (not what waits in the line buffer).
fn drawn(s: &State) -> Vec<String> {
    let mut rows = s.ui.messages.clone();
    let t = &s.ui.text;
    rows.push(
        t.transcript_line
            .iter()
            .map(|&b| crate::ui::wide::latin_char(b))
            .collect(),
    );
    rows
}

/// What waits in the line buffer, not drawn yet.
fn undrawn(s: &State) -> String {
    let t = &s.ui.text;
    t.line[usize::from(t.line_start)..usize::from(t.line_len)]
        .iter()
        .map(|&b| crate::ui::wide::latin_char(b))
        .collect()
}

/// A conversation with NPC 3 whose greeting is `greeting`, in `s`, run until it waits at its
/// first more prompt; the engine (still waiting) and what was said by then.
fn at_more_prompt(s: State, greeting: &[u8]) -> (Engine, Vec<Utterance>) {
    let mut body = greeting.to_vec();
    body.push(0xb6);
    wait_at_more(s, talk_files(&body))
}

/// A conversation with NPC 3 over `files`, in `s` with voices on, run until it waits at its
/// first more prompt; the engine (still waiting) and what was said by then.
fn wait_at_more(mut s: State, files: Files) -> (Engine, Vec<Utterance>) {
    speech::set_enabled(&mut s, true);
    let mut e = Engine::with_game(s, files, |st, io| async move {
        begin(&super::Cx::new(&st, &io), 0, 3).await;
    });
    for _ in 0..8 {
        e.tick(&[], 16);
    }
    assert!(!e.finished(), "waiting at a more prompt");
    let said = e.take_speech();
    (e, said)
}

/// Presses the more key until the conversation ends; what was said meanwhile.
fn page_on(e: &mut Engine) -> Vec<Utterance> {
    let mut said = Vec::new();
    for _ in 0..16 {
        if e.finished() {
            break;
        }
        e.tick(&[InputEvent::Key(Key::Space)], 16);
        said.extend(e.take_speech());
    }
    assert!(e.finished());
    said
}

#[test]
fn the_more_prompt_lets_out_only_what_is_drawn_and_the_next_flush_brings_the_rest() {
    // Rows end at newlines: three rows before the prompt (the look line, "One" and "Two"), and
    // the newline after "Two" waits at it before "Two" is drawn. So only "One" was said; "Two"
    // waits in the line, and comes with the rest after the more key.
    let mut s = state(&[]);
    s.ui.text.more_rows = 3;
    let (mut e, first) = at_more_prompt(s, b"One\nTwo\nThree");
    let screen = drawn(&e.state());
    assert_eq!(said(&first), [(0, 0, 3, "One\n")]);
    assert_eq!(
        screen.last().map(String::as_str),
        Some(""),
        "no part of the row drawn"
    );
    assert_eq!(screen[screen.len() - 2], "One", "the last row drawn");
    assert_eq!(undrawn(&e.state()), "Two");
    assert_eq!(said(&page_on(&mut e)), [(1, 0, 3, "Two\nThree")]);
}

#[test]
fn at_a_more_prompt_inside_a_wrapped_text_what_is_said_is_exactly_the_rows_drawn() {
    // The game's own message scroll: 17 cells wide, words wrapped. The greeting runs over the
    // rows left before the prompt; at the prompt the words said are those of the greeting's rows
    // drawn, the row waiting in the line is not said, and it opens what is said next.
    let mut s = state(&[]);
    let tables = s.ui.tables.clone();
    s.ui.text.main_windows(&tables);
    s.ui.text.more_rows = 5;
    let greeting = b"Hail traveller, the road to the north is long and the nights there are \
        cold, so rest by my fire awhile.";
    let (mut e, first) = at_more_prompt(s, greeting);
    let words = |t: &str| t.split_whitespace().map(String::from).collect::<Vec<_>>();
    let screen = words(&drawn(&e.state()).join(" "));
    let from = screen
        .iter()
        .position(|w| w == "Hail")
        .expect("a greeting row drawn");
    assert_eq!(first.len(), 1, "one flush at the prompt");
    assert_eq!(
        words(&first[0].text),
        screen[from..],
        "what is said is what is drawn"
    );
    let waiting = undrawn(&e.state());
    assert!(!waiting.trim().is_empty(), "a row waits in the line");
    let rest = page_on(&mut e);
    assert_eq!(rest.len(), 1);
    assert!(
        rest[0].text.starts_with(&waiting),
        "the row held back opens the next utterance"
    );
    let all = format!("{} {}", first[0].text, rest[0].text);
    assert_eq!(
        words(&all),
        words(std::str::from_utf8(greeting).unwrap_or_default()),
        "nothing lost or said twice"
    );
}

#[test]
fn with_voices_off_nothing_is_queued() {
    let body = b"Hi $P.\xf7Plain \xefname\xf6and nested.\xee\xb6";
    let (u, s) = talk_in(state(&[]), body, "name\n", false);
    assert!(u.is_empty());
    assert_eq!(s.speech, speech::SpeechTap::default());
    // The same conversation draws the same screen with voices on.
    let (_, on) = talk(body, "name\n");
    assert_eq!(scroll(&s), scroll(&on));
    assert_eq!(s.ui.messages, on.ui.messages);
}

#[test]
fn only_the_message_scroll_is_kept_and_only_with_the_flag_up() {
    let mut s = state(&[]);
    speech::set_enabled(&mut s, true);
    let (_, mut s) = run!(s, Files::default(), NONE, |cx| {
        let was = cx.st(|s| speech::raise(s, Kind::Speech));
        crate::ui::panels::select_window(cx.state, cx.io, WindowId::Status).await;
        seams::print(cx, b"status ").await;
        crate::ui::panels::select_window(cx.state, cx.io, WindowId::Messages).await;
        seams::print(cx, b"scroll").await;
        cx.st(|s| speech::lower(s, was));
        seams::print(cx, b" engine").await;
        let was = cx.st(|s| speech::raise(s, Kind::Look));
        seams::print(cx, b" look").await;
        cx.st(|s| speech::lower(s, was));
        cx.st(speech::flush);
    });
    assert_eq!(said(&speech::take(&mut s)), [(0, 0, 0, "scroll")]);
}

#[test]
fn the_queue_keeps_the_newest_utterances_and_an_endless_text_is_cut() {
    let mut s = State::empty();
    speech::set_enabled(&mut s, true);
    speech::raise(&mut s, Kind::Speech);
    for _ in 0..speech::QUEUE + 6 {
        speech::capture(&mut s, true, crate::ui::Language::English, i32::from(b'a'));
        speech::flush(&mut s);
    }
    let u = speech::take(&mut s);
    assert_eq!(u.len(), speech::QUEUE);
    assert_eq!((u[0].seq, u.last().map(|u| u.seq)), (6, Some(69)));
    for _ in 0..speech::TEXT_MAX + 1 {
        speech::capture(&mut s, true, crate::ui::Language::English, i32::from(b'b'));
    }
    speech::flush(&mut s);
    let u = speech::take(&mut s);
    assert_eq!(u.len(), 2);
    assert_eq!(u[0].text.chars().count(), speech::TEXT_MAX);
    assert_eq!(u[1].text, "b");
    // Off drops what was kept.
    speech::capture(&mut s, true, crate::ui::Language::English, i32::from(b'c'));
    speech::set_enabled(&mut s, false);
    speech::set_enabled(&mut s, true);
    speech::raise(&mut s, Kind::Speech);
    speech::flush(&mut s);
    assert!(speech::take(&mut s).is_empty());
}

#[test]
fn a_sentinel_said_reaches_only_take_speech() {
    const SENTINEL: &str = "Qx7Sentinel";
    let mut body = format!("Hi {SENTINEL}.").into_bytes();
    body.push(0xb6);
    let mut s = state(&[]);
    speech::set_enabled(&mut s, true);
    let mut e = Engine::with_game(s, talk_files(&body), |st, io| async move {
        begin(&super::Cx::new(&st, &io), 0, 3).await;
    });
    for _ in 0..16 {
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let log = e.take_log().join("\n");
    let tap = format!("{:?}", e.state().speech);
    let u = e.take_speech();
    assert_eq!(u.len(), 1);
    assert!(u[0].text.contains(SENTINEL), "the positive control");
    assert!(!format!("{u:?}").contains(SENTINEL), "an utterance's Debug");
    assert!(!tap.contains(SENTINEL), "the tap's Debug");
    assert!(!log.contains(SENTINEL), "the engine log");
    let stubs = format!("{:?}", e.stub_hits());
    assert!(!stubs.contains(SENTINEL), "the stub report");
}

// --- Korean speech (spec 09 Order 6) ------------------------------------------------------------

/// The made-up Korean line of the greeting's run, and of the look text.
const KO_GREETING: &str = "몰칸 브레딘 숩.";
const KO_LOOK: &str = "누벨 한";

/// The pack of `script` (NPC 3's, the locale tests' archive) in `language` holding only the runs
/// `lines` gives a line for: every other run prints its English, in the English grammar.
fn pack_of(script: &[u8], language: Language, lines: &[(&str, &str)]) -> Pack {
    let units = ScriptUnits::build(b'a', 3, script);
    let lines: BTreeMap<&str, &str> = lines.iter().copied().collect();
    let all: BTreeMap<&str, &str> = units
        .runs
        .iter()
        .map(|r| {
            let t = r.unit.text.as_str();
            (t, lines.get(t).copied().unwrap_or(t))
        })
        .collect();
    let full = tests_locale::pack_for(script, language, &all);
    let runs = units
        .runs
        .iter()
        .filter(|r| lines.contains_key(r.unit.text.as_str()))
        .map(|r| {
            let key = (b'a', 3, r.unit.offset);
            (key, full.run(key).cloned().expect("the pack has the run"))
        });
    Pack::new(language, "test/1", full.converse_base, runs).unwrap()
}

/// A conversation with NPC 3 over `script` (the locale tests' archive) typing `keys`, voices on,
/// with `pack` installed; the utterances queued.
fn talk_packed(script: &[u8], pack: Pack, keys: &str) -> Vec<Utterance> {
    let f = tests_locale::talk_files(script);
    let mut s = state(&[]);
    locale::install(&mut s, &f, Rc::new(pack), None).expect("the pack checks");
    speech::set_enabled(&mut s, true);
    let (_, mut s) = run!(s, f, chars(keys), |cx| begin(cx, 0, 3).await);
    speech::take(&mut s)
}

/// (seq, turn, text, lang) of each utterance.
fn said_in(u: &[Utterance]) -> Vec<(u32, u32, &str, Lang)> {
    u.iter()
        .map(|u| (u.seq, u.turn, u.text.as_str(), u.lang))
        .collect()
}

#[test]
fn a_run_the_korean_pack_has_is_said_as_its_korean_and_one_it_lacks_in_english() {
    // The pack has the greeting's run (and the look text, which is not said), not the answer's:
    // the greeting is said as the pack's line exactly, marked Korean; the answer as its English,
    // marked English.
    let script = tests_locale::script(b"Zorvel quint.\xf7Plosk varrow.\xb6");
    let ko = pack_of(
        &script,
        Language::Korean,
        &[("a woman", KO_LOOK), ("Zorvel quint.", KO_GREETING)],
    );
    let u = talk_packed(&script, ko, "x\n");
    assert_eq!(
        said_in(&u),
        [
            (0, 0, KO_GREETING, Lang::Ko),
            (1, 1, "Plosk varrow.", Lang::En)
        ]
    );
    assert!(u.iter().all(|u| !u.text.contains(KO_LOOK)));
    // The codes the page's voice worker is told.
    assert_eq!((Lang::Ko.code(), Lang::En.code()), ("ko", "en"));
    // The identity pack in the English grammar: every run its own text, and both English.
    let units = ScriptUnits::build(b'a', 3, &script);
    let identity: Vec<(&str, &str)> = units
        .runs
        .iter()
        .map(|r| (r.unit.text.as_str(), r.unit.text.as_str()))
        .collect();
    let en = pack_of(&script, Language::English, &identity);
    let u = talk_packed(&script, en, "x\n");
    assert_eq!(
        said_in(&u),
        [
            (0, 0, "Zorvel quint.", Lang::En),
            (1, 1, "Plosk varrow.", Lang::En)
        ]
    );
}

#[test]
fn an_utterance_a_korean_run_is_part_of_is_korean() {
    // Three runs with no wait between them (two assignments part them): English, the pack's
    // Korean, English. They are one utterance, and it is Korean, as any part of it is.
    let body =
        b"Ennor plast.\xa6\x00\xb2\xa8\x07\xa7Zorvel quint.\xa6\x00\xb2\xa8\x07\xa7Dabbet wun.\xb6";
    let script = tests_locale::script(body);
    let ko = pack_of(
        &script,
        Language::Korean,
        &[("a woman", KO_LOOK), ("Zorvel quint.", KO_GREETING)],
    );
    let u = talk_packed(&script, ko, "");
    let whole = format!("Ennor plast.{KO_GREETING}Dabbet wun.");
    assert_eq!(said_in(&u), [(0, 0, whole.as_str(), Lang::Ko)]);
    // The same pack less that run: the utterance is all English, and so is its language.
    let ko = pack_of(&script, Language::Korean, &[("a woman", KO_LOOK)]);
    let u = talk_packed(&script, ko, "");
    assert_eq!(
        said_in(&u),
        [(0, 0, "Ennor plast.Zorvel quint.Dabbet wun.", Lang::En)]
    );
}

#[test]
fn at_a_more_prompt_between_a_korean_run_and_an_english_one_each_part_has_its_own_language() {
    // Three rows before the prompt, as in the more prompt test above: the look line, the first
    // run's row, and the second run's first row, whose newline waits at it before that row is
    // drawn. So the first run is said at the prompt, and the second, its row held back, after the
    // key: each part with the language of its own characters, not of what the tap held.
    let split = |body: &[u8]| {
        let script = tests_locale::script(body);
        let ko = pack_of(
            &script,
            Language::Korean,
            &[
                ("a woman", KO_LOOK),
                ("Zorvel\n", "몰칸\n"),
                ("Zorvel\nquint.", "몰칸\n브레딘."),
            ],
        );
        let f = tests_locale::talk_files(&script);
        let mut s = state(&[]);
        locale::install(&mut s, &f, Rc::new(ko), None).expect("the pack checks");
        s.ui.text.more_rows = 3;
        let (mut e, first) = wait_at_more(s, f);
        let rest = page_on(&mut e);
        (first, rest)
    };
    // The Korean run drawn, the English one held.
    let (first, rest) = split(b"Zorvel\n\xa6\x00\xb2\xa8\x07\xa7Plosk\nvarrow.\xb6");
    assert_eq!(said_in(&first), [(0, 0, "몰칸\n", Lang::Ko)]);
    assert_eq!(said_in(&rest), [(1, 0, "Plosk\nvarrow.", Lang::En)]);
    // The English run drawn, the Korean one held.
    let (first, rest) = split(b"Plosk\n\xa6\x00\xb2\xa8\x07\xa7Zorvel\nquint.\xb6");
    assert_eq!(said_in(&first), [(0, 0, "Plosk\n", Lang::En)]);
    assert_eq!(said_in(&rest), [(1, 0, "몰칸\n브레딘.", Lang::Ko)]);
}

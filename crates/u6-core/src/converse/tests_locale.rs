//! The locale layer's conversation runs (spec 08 Design 2, slice K1) on hand-built scripts: a
//! pack built from the script's own catalog, every run given a made-up Korean line, then the
//! same conversation with and without it. In each case of Design 2's list (a run before and
//! after an assignment that changes the variable it prints; a false `a1` skip that lands on
//! `a2`, and an `a3` skip; a `b0` jump into the middle of a run; a `print_op d2` and an
//! `assign d2` table string; `*` inside a run) the interpreter reads exactly the bytes, ops and
//! inputs it reads in English, in the same order (the reads the disassembler's trace records),
//! ends with the same variables, and shows the Korean lines where English shows the runs. All
//! text here is the test's own.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::begin;
use super::disasm::catalog::ScriptUnits;
use super::disasm::trace::{self, Event};
use super::tests::{chars, files, lib, packed, run, state};
use crate::engine::{Engine, InputEvent};
use crate::files::Files;
use crate::locale::{self, Pack, PackError, RunText, fingerprint};
use crate::state::State;
use crate::ui::Language;
use crate::ui::wide::cell_char;

/// The script header of NPC 3 ("Ann", whose look text is "a woman"); its body follows.
pub(super) const HEADER: &[u8] = b"\xff\x03Ann\xf1a woman\xf2";

pub(super) fn script(body: &[u8]) -> Vec<u8> {
    [HEADER, body].concat()
}

/// NPC 3's script as item 3 of the conversation archive the synthetic messages name.
pub(super) fn talk_files(script: &[u8]) -> Files {
    let mut a: Vec<Option<Vec<u8>>> = vec![None; 4];
    a[3] = Some(packed(script));
    files(&[("talkfile.a", lib(&a))])
}

/// A pack of every run of `script`'s catalog (as NPC 3's, item 3 of `a`), each run's
/// translation `lines` gives for its text; a run it has no line for fails the test.
pub(super) fn pack_for(script: &[u8], language: Language, lines: &BTreeMap<&str, &str>) -> Pack {
    let units = ScriptUnits::build(b'a', 3, script);
    assert_eq!(units.refused, None, "the script disassembles");
    let runs = units.runs.iter().map(|r| {
        let line = lines
            .get(r.unit.text.as_str())
            .unwrap_or_else(|| panic!("no line for the run {:?}", r.unit.text));
        (
            (b'a', 3, r.unit.offset),
            RunText {
                len: r.unit.len,
                hash: r.unit.hash,
                text: line.as_bytes().to_vec(),
            },
        )
    });
    Pack::new(
        language,
        "test/1",
        fingerprint([(b'a', 3, units.hash)]),
        runs,
    )
    .unwrap()
}

/// The reads of a trace that are the interpreter's control flow: every one but the cells put.
pub(super) fn flow(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter(|e| !matches!(e, Event::Emit { .. }))
        .map(|e| format!("{e:?}"))
        .collect()
}

/// What the message scroll shows: its finished lines, then the one being written, each cell as
/// its character (Hangul included).
pub(super) fn screen(s: &State) -> String {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let mut last: String = t.transcript_line.iter().map(|&c| cell_char(c)).collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&c| cell_char(c)));
    lines.push(last);
    lines.join("\n")
}

/// A conversation with NPC 3 over `script` typing `keys`, with `pack` installed (or none): its
/// control flow (the trace's reads) and the state after it.
fn talk(script: &[u8], pack: Option<&Pack>, keys: &str) -> (Vec<String>, State) {
    let f = talk_files(script);
    let mut s = state(&[]);
    if let Some(p) = pack {
        locale::install(&mut s, &f, Rc::new(p.clone()), None).expect("the pack checks");
        assert!(s.ui.text.wide, "an installed pack turns the assembler on");
    }
    trace::start(false);
    let (_, s) = run!(s, f, chars(keys), |cx| begin(cx, 0, 3).await);
    (flow(&trace::stop()), s)
}

/// The case `body` typed `keys` in English and with the Korean pack of `lines`: the same reads
/// in the same order and the same variables; the Korean screen, which is returned with the
/// English one, and the pack's counts.
fn both(body: &[u8], keys: &str, lines: &[(&str, &str)]) -> (String, String, u32, u32) {
    let script = script(body);
    let lines: BTreeMap<&str, &str> = lines.iter().copied().collect();
    let pack = pack_for(&script, Language::Korean, &lines);
    let (en_flow, en) = talk(&script, None, keys);
    let (ko_flow, ko) = talk(&script, Some(&pack), keys);
    assert!(!en_flow.is_empty());
    assert_eq!(ko_flow, en_flow, "the Korean pack reads what English reads");
    assert_eq!(
        ko.converse.ints, en.converse.ints,
        "the same word variables"
    );
    assert_eq!(ko.converse.buffers, en.converse.buffers, "the same strings");
    assert_eq!(
        ko.ui.text.language,
        Language::English,
        "the grammar is back"
    );
    (
        screen(&en).trim_end().to_string(),
        screen(&ko).trim_end().to_string(),
        ko.locale.hits,
        ko.locale.misses,
    )
}

#[test]
fn a_run_before_and_after_an_assignment_prints_the_variable_as_it_is_then() {
    // `#0` is 0 before op a6 sets it to 7.
    let (en, ko, hits, misses) = both(
        b"Now #0.\xa6\x00\xb2\xa8\x07\xa7Then #0.\xb6",
        "",
        &[
            ("a woman", "여인"),
            ("Now #0.", "지금은 #0."),
            ("Then #0.", "이제는 #0."),
        ],
    );
    assert!(en.contains("a woman\nNow 0.Then 7."), "{en}");
    assert!(ko.contains("여인\n지금은 0.이제는 7."), "{ko}");
    assert!(!ko.contains("Now") && !ko.contains("Then"));
    assert_eq!((hits, misses), (3, 0));
}

#[test]
fn a_false_if_lands_after_its_endif_and_an_else_is_skipped_to_its_endif() {
    // a1 0: false, past the first a2 ("B." is not read); a1 1: true, "D.", then a3 skips "E.".
    let body = b"A.\xa1\x00\xa7B.\xa2C.\xa1\x01\xa7D.\xa3E.\xa2F.\xb6";
    let lines = [
        ("a woman", "여인"),
        ("A.", "가."),
        ("B.", "나."),
        ("C.", "다."),
        ("D.", "라."),
        ("E.", "마."),
        ("F.", "바."),
    ];
    let (en, ko, hits, _) = both(body, "", &lines);
    assert!(en.ends_with("A.C.D.F."), "{en}");
    assert!(ko.ends_with("가.다.라.바."), "{ko}");
    assert_eq!(hits, 5, "the look text and the four runs read");
}

#[test]
fn a_jump_into_the_middle_of_a_run_puts_its_second_part_and_a_whole_read_puts_both() {
    // "one" answers with a b0 into the middle of the run after the block, which a non-matching
    // input reaches from its start (the block's no-match landing): the catalog splits it there.
    let mut body = b"Hi.\xf7\xefone\xf6\xb0".to_vec();
    let at = (HEADER.len() + body.len() + 4 + 1 + b"First part. ".len()) as u32;
    body.extend_from_slice(&at.to_le_bytes());
    body.extend_from_slice(b"\xeeFirst part. Second part.\xb6");
    let units = ScriptUnits::build(b'a', 3, &script(&body));
    let parts: Vec<_> = units.runs.iter().filter(|r| r.part_of.is_some()).collect();
    assert_eq!(parts.len(), 2, "the run is split at the jump's target");
    assert_eq!(parts[1].unit.offset, at);
    let lines = [
        ("a woman", "여인"),
        ("Hi.", "어서 오시오."),
        ("First part. ", "첫째로 "),
        ("Second part.", "둘째로."),
    ];
    let (en, ko, hits, _) = both(&body, "one\n", &lines);
    assert!(
        en.ends_with("Second part.") && !en.contains("First part"),
        "{en}"
    );
    assert!(ko.ends_with("둘째로.") && !ko.contains("첫째로"), "{ko}");
    assert_eq!(hits, 3);
    let (en, ko, hits, _) = both(&body, "x\n", &lines);
    assert!(en.ends_with("First part. Second part."), "{en}");
    assert!(ko.ends_with("첫째로 둘째로."), "{ko}");
    assert_eq!(hits, 4, "both parts, one after the other");
}

#[test]
fn table_strings_printed_and_copied_stay_english_between_korean_runs() {
    // b5 d2 prints string 1 of the table at 0x80; a6 d2 copies string 0 into slot 1, which `$1`
    // prints. A pack without table strings (K1's) leaves them English.
    let table = 0x80u32;
    let addr = |a: u32| [&[0xd2][..], &a.to_le_bytes()].concat();
    let mut body = b"Pick \xb5".to_vec();
    body.extend(addr(table));
    body.extend_from_slice(b"\x01\xa7 now.\xa6\x01\xb3\xa8");
    body.extend(addr(table));
    body.extend_from_slice(b"\x00\xa7Kept $1.\xb6");
    body.resize(table as usize - HEADER.len(), 0);
    body.extend_from_slice(b"red\0blue\0");
    let lines = [
        ("a woman", "여인"),
        ("Pick ", "고르시오 "),
        (" now.", " 지금."),
        ("Kept $1.", "남은 것은 $1."),
    ];
    let (en, ko, hits, _) = both(&body, "", &lines);
    assert!(en.ends_with("Pick blue now.Kept red."), "{en}");
    assert!(ko.ends_with("고르시오 blue 지금.남은 것은 red."), "{ko}");
    assert_eq!(hits, 4);
}

#[test]
fn a_star_inside_a_run_waits_for_a_key_where_english_waits() {
    // The key after the wait is the same one; the next input op then reads "x".
    let body = b"One*Two.\xf7\xefx\xf6Done.\xb6\xee";
    let lines = [
        ("a woman", "여인"),
        ("One*Two.", "하나*둘."),
        ("Done.", "끝."),
    ];
    let (en, ko, hits, _) = both(body, " x\n", &lines);
    assert!(en.ends_with("Done."), "{en}");
    assert!(
        ko.contains("하나") && ko.contains("둘.") && ko.ends_with("끝."),
        "{ko}"
    );
    assert_eq!(hits, 3);
}

#[test]
fn a_run_the_pack_lacks_or_whose_bytes_changed_prints_english_and_is_counted() {
    let script = script(b"Kept.\xa6\x00\xb2\xa8\x07\xa7Left.\xb6");
    let lines: BTreeMap<&str, &str> = [("a woman", "여인"), ("Kept.", "남김."), ("Left.", "둠.")]
        .into_iter()
        .collect();
    let full = pack_for(&script, Language::Korean, &lines);
    let units = ScriptUnits::build(b'a', 3, &script);
    let left = units.runs.last().unwrap().unit.offset;
    // The same pack less its last run.
    let part = Pack::new(
        Language::Korean,
        "test/1",
        full.converse_base,
        units
            .runs
            .iter()
            .filter(|r| r.unit.offset != left)
            .map(|r| {
                (
                    (b'a', 3, r.unit.offset),
                    full.run((b'a', 3, r.unit.offset)).unwrap().clone(),
                )
            }),
    )
    .unwrap();
    let (_, s) = talk(&script, Some(&part), "");
    let shown = screen(&s);
    assert!(shown.ends_with("남김.Left."), "{shown}");
    assert_eq!((s.locale.hits, s.locale.misses), (2, 1));
    // A run whose bytes in the buffer differ from the pack's (an op patched them, or the buffer
    // holds another script) is a miss, though the pack was checked at load.
    let f = talk_files(&script);
    let mut s = state(&[]);
    locale::install(&mut s, &f, Rc::new(full), None).unwrap();
    trace::start(false);
    let mut e = Engine::with_game(s, f, |st, io| async move {
        let cx = super::Cx::new(&st, &io);
        super::script::load_script(
            &mut st.borrow_mut(),
            io.files(),
            3,
            super::script::ScriptDst::Main,
        );
        let at = HEADER.len() + 1;
        st.borrow_mut().converse.script[at] = b'X';
        st.borrow_mut().converse.pc = HEADER.len() as u16;
        super::run::run_text(&cx, 3, crate::speech::Kind::Speech).await;
    });
    for _ in 0..50 {
        e.tick(&[], 16);
    }
    trace::stop();
    assert!(
        screen(&e.state()).starts_with("KXpt."),
        "{}",
        screen(&e.state())
    );
    assert_eq!(e.state().locale.misses, 1);
}

#[test]
fn the_identity_pack_with_english_grammar_shows_and_reads_exactly_english() {
    // Every run's translation is its own text: with `wide` on, the screen, every cell put and
    // every read are English's.
    let body = b"Hi $P, #0 coin/s.\xf7\xefname\xf6I am $N*.\xb6\xee";
    let script = script(body);
    let units = ScriptUnits::build(b'a', 3, &script);
    let lines: BTreeMap<&str, &str> = units
        .runs
        .iter()
        .map(|r| (r.unit.text.as_str(), r.unit.text.as_str()))
        .collect();
    let pack = pack_for(&script, Language::English, &lines);
    let cells = |pack: Option<&Pack>| {
        let f = talk_files(&script);
        let mut s = state(&[]);
        if let Some(p) = pack {
            locale::install(&mut s, &f, Rc::new(p.clone()), None).unwrap();
        }
        trace::start(false);
        let keys: Vec<InputEvent> = chars("name\n ");
        let (_, s) = run!(s, f, keys, |cx| begin(cx, 0, 3).await);
        let events = trace::stop();
        (format!("{events:?}"), screen(&s), s.locale.hits)
    };
    let (en_events, en_screen, _) = cells(None);
    let (id_events, id_screen, hits) = cells(Some(&pack));
    assert_eq!(id_screen, en_screen);
    assert_eq!(id_events, en_events, "every read and every cell, in order");
    assert!(hits >= 3);
}

#[test]
fn a_pack_for_other_game_files_is_refused_and_changes_nothing() {
    let mine = script(b"Hi.\xb6");
    let lines: BTreeMap<&str, &str> = [("a woman", "여인"), ("Hi.", "어서.")]
        .into_iter()
        .collect();
    let pack = pack_for(&mine, Language::Korean, &lines);
    let other = talk_files(&script(b"Ho.\xb6"));
    let mut s = state(&[]);
    let before = s.clone();
    let err = locale::install(&mut s, &other, Rc::new(pack.clone()), None).unwrap_err();
    assert_eq!(err.part, locale::Part::Converse);
    assert_eq!(s, before, "nothing changed");
    // And no archive at all.
    assert!(locale::install(&mut s, &Files::default(), Rc::new(pack), None).is_err());
    assert_eq!(s, before);
    // Building a pack checks its keys as the reader does.
    let bad = Pack::new(
        Language::Korean,
        "test/1",
        0,
        [(
            (b'a', 3, 0),
            RunText {
                len: 0,
                hash: 0,
                text: Vec::new(),
            },
        )],
    );
    assert_eq!(bad, Err(PackError::Key));
}

#[test]
fn a_new_game_process_keeps_the_installed_pack() {
    // `u` runs again and a new `game` takes the state's place (main_loop's `take_over`): the
    // pack, its Hangul table and the assembler stay on; the new state's own text layer is reset.
    let mine = script(b"Hi.\xb6");
    let lines: BTreeMap<&str, &str> = [("a woman", "여인"), ("Hi.", "어서.")]
        .into_iter()
        .collect();
    let pack = Rc::new(pack_for(&mine, Language::Korean, &lines));
    let f = talk_files(&mine);
    let mut s = state(&[]);
    let font = u6_formats::font::HangulFont::from_glyphs(vec![[0u8; 8]; 11172]).unwrap();
    locale::install(&mut s, &f, pack.clone(), Some(Rc::new(font))).unwrap();
    let mut e = Engine::with_game(s, f, |st, io| async move {
        let fresh = state(&[]);
        assert!(!fresh.ui.text.wide && fresh.locale.pack.is_none());
        crate::main_loop::take_over(&mut st.borrow_mut(), fresh, &io);
    });
    e.tick(&[], 16);
    let s = e.state();
    assert!(s.locale.pack.as_ref().is_some_and(|p| Rc::ptr_eq(p, &pack)));
    assert!(s.ui.text.wide && s.ui.hangul.is_some());
}

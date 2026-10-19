//! The locale layer's table strings and executable strings (spec 08 Design 2, slice K2) on
//! hand-built scripts and synthetic messages (all text here is the test's own):
//!
//! - a `print_op d2` and an `assign d2` table string: the Korean follows English's control flow
//!   (the interpreter reads the same bytes, ops and inputs in the same order, ends with the same
//!   variables, and a comparison of the copied slot reads the English), the window shows the
//!   pack's text; a copy whose translation the slot cannot hold stays English and is counted;
//!   the identity pack reads and puts exactly as English;
//! - executable strings: a catalogued format, message and label (`$G`, `$T`) from the pack, with
//!   logic reading the English; what an escape generates (`%s`, `#n`, `$n`) never looked up; a
//!   string under three bytes, and one the catalogue lacks, never substituted; one the pack lacks
//!   counted; a pack whose format changes its `%` directives refused at install.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::begin;
use super::disasm::catalog::{ScriptUnits, hash64};
use super::disasm::trace;
use super::tests::{chars, run, state};
use super::tests_locale::{HEADER, flow, pack_for, screen, script, talk_files};
use super::vars::StrRef;
use crate::engine::InputEvent;
use crate::files::Files;
use crate::locale::strings::{Catalogue, Kind};
use crate::locale::{self, Pack, StringText, TableText};
use crate::state::State;
use crate::ui::Language;
use crate::ui::printf::{self, Arg};

/// The pack of `script` (NPC 3's, item 3 of `a`): every run as `lines` has it, every table
/// string as `tables` has it (by its English), each with the catalog's capacity; `text` gives a
/// table string another translation than its line.
fn pack_with_tables(
    script: &[u8],
    language: Language,
    lines: &BTreeMap<&str, &str>,
    tables: &BTreeMap<&str, String>,
    capacity: impl Fn(Option<u32>) -> Option<u32>,
) -> Pack {
    let units = ScriptUnits::build(b'a', 3, script);
    assert_eq!(units.refused, None, "the script disassembles");
    let strings = units.strings.iter().map(|t| {
        let text = tables
            .get(t.unit.text.as_str())
            .unwrap_or_else(|| panic!("no line for the table string {:?}", t.unit.text));
        (
            (b'a', 3, t.unit.offset),
            TableText {
                len: t.unit.len,
                hash: t.unit.hash,
                capacity: capacity(t.capacity),
                text: text.as_bytes().to_vec(),
            },
        )
    });
    pack_for(script, language, lines)
        .with_tables(strings)
        .expect("the table strings read")
}

/// A conversation with NPC 3 over `script` typing `keys`, with `pack` installed over the
/// catalogue `catalogue` (or no pack): its control flow and the state after it.
fn talk(
    script: &[u8],
    pack: Option<&Pack>,
    catalogue: &Catalogue,
    keys: &str,
) -> (Vec<String>, String, State) {
    talk_from(state(&[]), script, pack, catalogue, keys)
}

/// [`talk`] from the state `s`.
fn talk_from(
    mut s: State,
    script: &[u8],
    pack: Option<&Pack>,
    catalogue: &Catalogue,
    keys: &str,
) -> (Vec<String>, String, State) {
    let f = talk_files(script);
    if let Some(p) = pack {
        locale::install_catalogued(&mut s, &f, Rc::new(p.clone()), None, catalogue.clone())
            .expect("the pack checks");
    }
    trace::start(false);
    let (_, s) = run!(s, f, chars(keys), |cx| begin(cx, 0, 3).await);
    let events = trace::stop();
    (flow(&events), format!("{events:?}"), s)
}

/// The script of the table cases: `b5 d2` prints string 1 of the table at 0x80, `a6 d2` copies
/// string 0 into slot 1, which `$1` prints; after the player's words, the copied slot is compared
/// with them (`a1 $1 == $Z`).
fn table_script() -> Vec<u8> {
    let table = 0x80u32;
    let addr = |a: u32| [&[0xd2][..], &a.to_le_bytes()].concat();
    let mut body = b"Pick \xb5".to_vec();
    body.extend(addr(table));
    body.extend_from_slice(b"\x01\xa7 now.\xa6\x01\xb3\xa8");
    body.extend(addr(table));
    body.extend_from_slice(
        b"\x00\xa7Kept $1.\xf7\xef*\xf6\xa1\x01\xb3\x23\xb3\x86\xa7Same.\xa2\xb6\xee",
    );
    body.resize(table as usize - HEADER.len(), 0);
    body.extend_from_slice(b"red\0blue\0");
    script(&body)
}

fn lines() -> BTreeMap<&'static str, &'static str> {
    [
        ("a woman", "여인"),
        ("Pick ", "고르시오 "),
        (" now.", " 지금."),
        ("Kept $1.", "남은 것은 $1."),
        ("Same.", "같소."),
    ]
    .into_iter()
    .collect()
}

fn colours() -> BTreeMap<&'static str, String> {
    [("red", "빨강".to_string()), ("blue", "파랑".to_string())]
        .into_iter()
        .collect()
}

#[test]
fn a_printed_and_a_copied_table_string_show_korean_and_logic_reads_the_english() {
    let script = table_script();
    let pack = pack_with_tables(&script, Language::Korean, &lines(), &colours(), |c| c);
    let units = ScriptUnits::build(b'a', 3, &script);
    assert_eq!(
        units.strings.iter().map(|t| t.capacity).collect::<Vec<_>>(),
        [Some(99), None],
        "string 0 is copied into slot 1"
    );
    let none = Catalogue::default();
    let (en_flow, _, en) = talk(&script, None, &none, "red\n");
    let (ko_flow, _, ko) = talk(&script, Some(&pack), &none, "red\n");
    assert!(!en_flow.is_empty());
    assert_eq!(
        ko_flow, en_flow,
        "the Korean reads what English reads, in order"
    );
    assert_eq!(
        ko.converse.ints, en.converse.ints,
        "the same word variables"
    );
    assert_eq!(
        ko.converse.buffers, en.converse.buffers,
        "the slot holds the English for logic"
    );
    assert_eq!(
        ko.converse.shown,
        [(
            StrRef::Buffer(1),
            Language::Korean,
            "빨강".as_bytes().to_vec()
        )],
        "and shows the pack's"
    );
    let (en, ko) = (screen(&en), screen(&ko));
    assert!(
        en.contains("Pick blue now.Kept red.") && en.contains("Same."),
        "{en}"
    );
    assert!(
        ko.contains("고르시오 파랑 지금.남은 것은 빨강.") && ko.contains("같소."),
        "the comparison of the copied slot with the input holds as in English: {ko}"
    );
    assert!(!ko.contains("red") || ko.contains("say:red"), "{ko}");
    assert!(!ko.contains("blue"), "{ko}");
}

#[test]
fn the_table_counts_and_a_copy_too_long_for_its_slot_stays_english() {
    let script = table_script();
    let none = Catalogue::default();
    let pack = pack_with_tables(&script, Language::Korean, &lines(), &colours(), |c| c);
    let (_, _, s) = talk(&script, Some(&pack), &none, "red\n");
    assert_eq!(s.locale.tables.get(), [2, 0], "the print and the copy");
    // A pack that does not say the capacity, with a translation longer than the 99 bytes the
    // slot's buffer holds: the copy shows the English, counted as missed; the print is Korean.
    let mut long = colours();
    long.insert("red", "빨".repeat(34));
    let pack = pack_with_tables(&script, Language::Korean, &lines(), &long, |_| None);
    let (en_flow, _, _) = talk(&script, None, &none, "red\n");
    let (ko_flow, _, s) = talk(&script, Some(&pack), &none, "red\n");
    assert_eq!(ko_flow, en_flow);
    assert!(s.converse.shown.is_empty(), "nothing shown in its place");
    let shown = screen(&s);
    assert!(shown.contains("남은 것은 red."), "{shown}");
    assert!(shown.contains("파랑"), "{shown}");
    assert_eq!(s.locale.tables.get(), [1, 1]);
    // At 99 bytes it fits.
    let mut fits = colours();
    fits.insert("red", "빨".repeat(33));
    let pack = pack_with_tables(&script, Language::Korean, &lines(), &fits, |_| None);
    let (_, _, s) = talk(&script, Some(&pack), &none, "red\n");
    assert_eq!(s.locale.tables.get(), [2, 0]);
    assert!(
        screen(&s).replace('\n', "").contains(&"빨".repeat(33)),
        "the 99 bytes shown (across the rows they wrap to)"
    );
}

#[test]
fn the_identity_pack_of_runs_and_table_strings_reads_and_puts_exactly_as_english() {
    let script = table_script();
    let units = ScriptUnits::build(b'a', 3, &script);
    let lines: BTreeMap<&str, &str> = units
        .runs
        .iter()
        .map(|r| (r.unit.text.as_str(), r.unit.text.as_str()))
        .collect();
    let tables: BTreeMap<&str, String> = units
        .strings
        .iter()
        .map(|t| (t.unit.text.as_str(), t.unit.text.clone()))
        .collect();
    let pack = pack_with_tables(&script, Language::English, &lines, &tables, |c| c);
    let none = Catalogue::default();
    let (_, en_events, en) = talk(&script, None, &none, "red\n");
    let (_, id_events, id) = talk(&script, Some(&pack), &none, "red\n");
    assert_eq!(id_events, en_events, "every read and every cell, in order");
    assert_eq!(screen(&id), screen(&en));
    assert_eq!(id.locale.tables.get(), [2, 0]);
}

#[test]
fn a_copied_translation_survives_a_copy_onto_its_own_place_and_never_reaches_a_place_without_its_english()
 {
    // `a6 d2` copies string 0 ("red", translated) into slot 1; then slot 1 onto itself, onto slot
    // 0x20 (which points at slot 1's buffer too), onto slot 0x21 (no place: the write is dropped)
    // and onto `$G` (the form of address in the code: dropped too); `$W` is slot 0x20, and `$K`
    // points nowhere, as 0x21 does.
    let table = 0x80u32;
    let addr = [&[0xd2][..], &table.to_le_bytes()].concat();
    let mut body = b"\xa6\x01\xb3\xa8".to_vec();
    body.extend(&addr);
    body.extend_from_slice(b"\x00\xa7\xa6\x01\xb3\xa8\x01\xb3Self $1.");
    body.extend_from_slice(b"\xa6\x20\xb3\xa8\x01\xb3Alias $W.");
    body.extend_from_slice(b"\xa6\x21\xb3\xa8\x01\xb3Null [$K].");
    body.extend_from_slice(b"\xa6\x10\xb3\xa8\x01\xb3G $G.\xb6");
    body.resize(table as usize - HEADER.len(), 0);
    body.extend_from_slice(b"red\0blue\0");
    let script = script(&body);
    let lines: BTreeMap<&str, &str> = [
        ("a woman", "여인"),
        ("Self $1.", "자신 $1."),
        ("Alias $W.", "별칭 $W."),
        ("Null [$K].", "빈 [$K]."),
        ("G $G.", "호칭 $G."),
    ]
    .into_iter()
    .collect();
    let pack = pack_with_tables(&script, Language::Korean, &lines, &colours(), |c| c);
    let none = Catalogue::default();
    let mut s0 = state(&[]);
    s0.converse.strs[0x20] = StrRef::Buffer(1);
    let (en_flow, _, en) = talk_from(s0.clone(), &script, None, &none, "");
    let (ko_flow, _, ko) = talk_from(s0, &script, Some(&pack), &none, "");
    assert_eq!(ko_flow, en_flow);
    assert_eq!(
        ko.converse.buffers, en.converse.buffers,
        "the English for logic"
    );
    let (en, ko_text) = (screen(&en), screen(&ko));
    assert!(
        en.contains("Self red.Alias red.Null [].G [Milord]."),
        "{en}"
    );
    assert!(
        ko_text.contains("자신 빨강."),
        "a copy onto itself keeps it: {ko_text}"
    );
    assert!(
        ko_text.contains("별칭 빨강."),
        "a copy onto a slot that points at the same buffer keeps it: {ko_text}"
    );
    assert!(
        ko_text.contains("빈 []."),
        "no place, no English written: nothing shown there or at another slot pointing nowhere: {ko_text}"
    );
    assert!(
        ko_text.contains("호칭 [Milord]."),
        "the form of address in the code keeps its own: {ko_text}"
    );
    assert_eq!(
        ko.converse.shown,
        [(
            StrRef::Buffer(1),
            Language::Korean,
            "빨강".as_bytes().to_vec()
        )],
        "only the buffer shows it"
    );
}

/// The catalogue of `entries` (as the engine makes it from a `game`, `Catalogue::from_game`)
/// and the pack's executable strings of `strings` (English, Korean).
fn strings(
    entries: &[(Kind, &str)],
    strings: &[(&str, &str)],
) -> (Catalogue, Vec<(u64, StringText)>) {
    let cat = Catalogue::from_entries(entries.iter().map(|&(k, t)| (k, t.as_bytes().to_vec())));
    let words = strings
        .iter()
        .map(|&(en, ko)| {
            (
                hash64(en.as_bytes()),
                StringText {
                    len: en.len() as u32,
                    text: ko.as_bytes().to_vec(),
                },
            )
        })
        .collect();
    (cat, words)
}

#[test]
fn the_conversations_own_messages_and_its_forms_of_address_show_the_packs_and_logic_reads_english()
{
    // The synthetic messages are `[Name]`: the frame's "you see" and the forms of address the
    // greeting prints through `$G` and `$T` (the hero is male, it is morning); after the
    // player's words, the slot `$G` is compared with them.
    let body = b"Hail, $G. Good $T.\xf7\xef*\xf6\xa1\x10\xb3\x23\xb3\x86\xa7Same.\xa2\xb6\xee";
    let script = script(body);
    let lines: BTreeMap<&str, &str> = [
        ("a woman", "여인"),
        ("Hail, $G. Good $T.", "어서 오시오, $G. 좋은 $T이오."),
        ("Same.", "같소."),
    ]
    .into_iter()
    .collect();
    let (cat, words) = strings(
        &[
            (Kind::Message, "[YouSeeFirst]"),
            (Kind::Label, "[Milord]"),
            (Kind::Label, "[Morning]"),
            (Kind::Message, "\nsay:"),
        ],
        &[
            ("[YouSeeFirst]", "그대가 보니 "),
            ("[Milord]", "나리"),
            ("[Morning]", "아침"),
        ],
    );
    let pack = pack_for(&script, Language::Korean, &lines)
        .with_strings(words)
        .unwrap();
    let pack = Pack::parse(&pack.to_bytes()).expect("the pack reads back");
    let none = Catalogue::default();
    let (en_flow, _, en) = talk(&script, None, &none, "[Milord]\n");
    let (ko_flow, _, ko) = talk(&script, Some(&pack), &cat, "[Milord]\n");
    assert_eq!(ko_flow, en_flow, "the same reads, the comparison included");
    let (en_text, ko_text) = (screen(&en), screen(&ko));
    assert!(
        en_text.contains("[YouSeeFirst]a woman") && en_text.contains("Same."),
        "{en_text}"
    );
    assert!(ko_text.contains("그대가 보니 여인"), "{ko_text}");
    assert!(
        ko_text.contains("어서 오시오, 나리. 좋은 아침이오."),
        "{ko_text}"
    );
    assert!(
        ko_text.contains("같소."),
        "`$G` compared with the input reads the English: {ko_text}"
    );
    assert!(ko_text.contains("say:"), "a string the pack lacks: English");
    // The frame's line and the two words from the pack; the prompt it lacks, missed (once per
    // input op).
    assert_eq!(ko.locale.texts.get(), [3, 1]);
}

/// Runs `body` (with `$cx: &Cx`) as a game over the synthetic state with `pack` installed over
/// `cat`; the screen after it, and the executable strings' counts.
macro_rules! printed {
    ($cat:expr, $words:expr, |$cx:ident| $body:expr) => {{
        let mut s = state(&[]);
        let pack = Pack::new(Language::Korean, "test/1", locale::fingerprint([]), [])
            .unwrap()
            .with_strings($words)
            .unwrap();
        locale::install_catalogued(&mut s, &Files::default(), Rc::new(pack), None, $cat)
            .expect("the pack checks");
        s.converse.buffers[1] = b"[Nothing]".to_vec();
        s.converse.strs[1] = StrRef::Buffer(1);
        s.converse.ints[0] = 123;
        let (_, s) = run!(s, Files::default(), Vec::<InputEvent>::new(), |$cx| {
            $body;
            crate::ui::print::put_raw($cx.state, $cx.io, -1).await;
        });
        (screen(&s), s.locale.texts.get())
    }};
}

fn exe() -> (Catalogue, Vec<(u64, StringText)>) {
    strings(
        &[
            (Kind::Format, "Look: %s!"),
            (Kind::Message, "[Nothing]"),
            (Kind::Message, "123"),
            (Kind::Message, "abc"),
            (Kind::Message, "[Undefined]"),
        ],
        &[
            ("Look: %s!", "보시오: %s!"),
            ("[Nothing]", "없음"),
            ("123", "백이십삼"),
            ("abc", "가나다"),
        ],
    )
}

#[test]
fn a_catalogued_format_message_or_line_game_code_prints_is_the_packs() {
    let (cat, words) = exe();
    let (shown, counts) = printed!(cat, words, |cx| {
        printf::printf(cx.state, cx.io, b"Look: %s!", &[Arg::Str(b"x")]).await;
        printf::puts(cx.state, cx.io, b"|[Nothing]").await;
        printf::puts(cx.state, cx.io, b"[Nothing]").await;
        crate::ui::print::put_raw_str(cx.state, cx.io, b"|").await;
        crate::ui::print::put_raw_str(cx.state, cx.io, b"[Nothing]").await;
    });
    assert_eq!(shown.trim_end(), "보시오: x!|[Nothing]없음|없음");
    assert_eq!(
        counts,
        [3, 0],
        "three from the pack; the uncatalogued line not counted"
    );
}

#[test]
fn what_an_escape_generates_is_never_looked_up() {
    // `%s` with a catalogued string the pack has, `$1` holding one, `#0` printing digits (which
    // no catalogued string is: it has no word): each is English, and nothing is counted (no
    // lookup was made).
    let (cat, words) = exe();
    let (shown, counts) = printed!(cat, words, |cx| {
        printf::printf(cx.state, cx.io, b"%s,", &[Arg::Str(b"[Nothing]")]).await;
        printf::printf(cx.state, cx.io, b"$1,#0", &[]).await;
        printf::puts(cx.state, cx.io, b",$1").await;
    });
    assert_eq!(shown.trim_end(), "[Nothing],[Nothing],123,[Nothing]");
    assert_eq!(counts, [0, 0]);
}

#[test]
fn a_short_string_or_one_the_catalogue_lacks_is_never_substituted_and_one_the_pack_lacks_is_counted()
 {
    let (cat, mut words) = exe();
    // The pack has a line for a string the catalogue lacks: never shown.
    words.push((
        hash64(b"Another row"),
        StringText {
            len: 11,
            text: "다른 줄".as_bytes().to_vec(),
        },
    ));
    let (shown, counts) = printed!(cat, words, |cx| {
        for line in [
            &b"ab"[..],
            b"|",
            b"abc",
            b"|",
            b"Another row",
            b"|",
            b"[Undefined]",
        ] {
            printf::printf(cx.state, cx.io, line, &[]).await;
        }
    });
    assert_eq!(shown.trim_end(), "ab|가나다|Another row|[Undefined]");
    assert_eq!(
        counts,
        [1, 1],
        "the three-byte one from the pack; the one it lacks, missed"
    );
}

#[test]
fn a_pack_whose_format_changes_its_directives_is_refused_at_install_and_changes_nothing() {
    let (cat, _) = exe();
    let (_, words) = strings(&[], &[("Look: %s!", "보시오: %d!")]);
    let pack = Pack::new(Language::Korean, "test/1", locale::fingerprint([]), [])
        .unwrap()
        .with_strings(words)
        .unwrap();
    let mut s = state(&[]);
    let before = s.clone();
    let err = locale::install_catalogued(&mut s, &Files::default(), Rc::new(pack), None, cat)
        .unwrap_err();
    assert_eq!(err.part, locale::Part::Strings);
    assert_eq!(s, before, "nothing changed");
}

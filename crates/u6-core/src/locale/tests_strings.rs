//! Slice K2 (spec 08): the pack's table strings and executable strings on synthetic packs and
//! archives (the test's own text): a round trip and a K1 pack's file unchanged, each refusal,
//! the capacity gate of the reader, the table strings' check with the runs, the `%` directives
//! of the executable strings checked against the catalogue, and the catalogue itself.

use super::strings::{Catalogue, Kind, directives};
use super::*;
use crate::converse::disasm::catalog::hash64;

const SENTINEL: &str = "Zq8StringSentinel";

/// A synthetic script: a header, a look text, a greeting, and a string table (two strings) after
/// the end.
fn script() -> Vec<u8> {
    b"\xff\x03Ann\xf1a woman\xf2Hello.\xb6red\0blue\0".to_vec()
}

/// Where the table's strings start in [`script`].
const RED: u32 = 21;
const BLUE: u32 = 25;

/// A U6Lib (u32 offsets) holding `script` as item 3, stored raw.
fn archive(script: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    for i in 0..4u32 {
        d.extend_from_slice(&(if i == 3 { 16u32 } else { 0 }).to_le_bytes());
    }
    d.extend_from_slice(&[0, 0, 0, 0]);
    d.extend_from_slice(script);
    d
}

fn table(text: &str, script: &[u8], at: u32, len: u32, capacity: Option<u32>) -> TableText {
    TableText {
        len,
        hash: hash64(&script[at as usize..(at + len) as usize]),
        capacity,
        text: text.as_bytes().to_vec(),
    }
}

fn string(english: &str, text: &str) -> (u64, StringText) {
    (
        hash64(english.as_bytes()),
        StringText {
            len: english.len() as u32,
            text: text.as_bytes().to_vec(),
        },
    )
}

/// A pack of the greeting, both table strings and two executable strings over `script`.
fn pack(script: &[u8]) -> Pack {
    Pack::new(
        Language::Korean,
        "test/1",
        fingerprint([(b'a', 3, hash64(script))]),
        [(
            (b'a', 3, 14),
            RunText {
                len: 6,
                hash: hash64(&script[14..20]),
                text: "안녕.".as_bytes().to_vec(),
            },
        )],
    )
    .unwrap()
    .with_tables([
        ((b'a', 3, RED), table("빨강", script, RED, 3, Some(99))),
        ((b'a', 3, BLUE), table("파랑", script, BLUE, 4, None)),
    ])
    .unwrap()
    .with_strings([
        string("You see %s.\n", "그대는 %s{을/를} 보오.\n"),
        string("Naught here", "아무것도"),
    ])
    .unwrap()
}

fn file(json: &str) -> Vec<u8> {
    [&b"U6LP\n"[..], json.as_bytes()].concat()
}

/// The JSON of a pack of no runs with `extra` fields after the K1 ones.
fn json_with(extra: &str) -> String {
    format!(
        "{{\"format\":\"u6.locale-pack/1\",\"language\":\"ko\",\"tool\":\"test/1\",\"base\":{{\"converse\":\"{}\"}},\"converse\":[]{extra}}}",
        format_args!("{:016x}", fingerprint([]))
    )
}

const TABLE: &str =
    "\"archive\":\"a\",\"item\":3,\"offset\":21,\"len\":3,\"hash\":\"0123456789abcdef\"";

#[test]
fn a_pack_with_table_and_game_strings_round_trips_and_a_runs_only_pack_is_k1s_file() {
    let s = script();
    let p = pack(&s);
    let bytes = p.to_bytes();
    assert_eq!(Pack::parse(&bytes), Ok(p.clone()));
    assert_eq!(
        (p.runs(), p.tables(), p.strings(), p.scripts()),
        (1, 2, 2, 1)
    );
    assert_eq!(p.table((b'a', 3, RED)).map(|t| t.capacity), Some(Some(99)));
    assert!(p.string(hash64(b"Naught here")).is_some());
    let text = String::from_utf8(bytes[5..].to_vec()).unwrap();
    assert!(text.contains("\"tables\":[") && text.contains("\"strings\":["));
    // A pack of runs only writes no new field: the K1 form, read as before.
    let k1 = Pack::new(Language::Korean, "test/1", 7, []).unwrap();
    let k1_bytes = k1.to_bytes();
    assert_eq!(
        k1_bytes,
        file(
            "{\"format\":\"u6.locale-pack/1\",\"language\":\"ko\",\"tool\":\"test/1\",\"base\":{\"converse\":\"0000000000000007\"},\"converse\":[]}"
        )
    );
    assert_eq!(Pack::parse(&k1_bytes), Ok(k1));
    // A strings-only pack names no script: its fingerprint is the empty set's.
    let only = Pack::parse(&file(&json_with(&format!(
        ",\"strings\":[{{\"hash\":\"{:016x}\",\"len\":7,\"text\":\"아무것도\"}}]",
        hash64(b"Naught here")
    ))))
    .unwrap();
    assert_eq!((only.scripts(), only.converse_base), (0, hash64(&[])));
    let lib = archive(&s);
    assert_eq!(only.check(|_| Some(lib.as_slice())), Ok(()));
}

#[test]
fn each_broken_rule_of_the_new_maps_refuses_the_pack() {
    let long = "가".repeat(MAX_STRING_TEXT_BYTES / 3 + 1);
    let cases: Vec<(String, PackError)> = vec![
        (
            format!(",\"tables\":[{{{TABLE},\"text\":\"a\",\"note\":1}}]"),
            PackError::Shape,
        ),
        (format!(",\"tables\":[{{{TABLE}}}]"), PackError::Shape),
        (
            ",\"strings\":[{\"hash\":\"0123456789abcdef\",\"len\":3}]".into(),
            PackError::Shape,
        ),
        (
            format!(",\"tables\":[{{{TABLE},\"capacity\":2,\"text\":\"abc\"}}]"),
            PackError::Capacity,
        ),
        (
            format!(",\"tables\":[{{{TABLE},\"capacity\":5,\"text\":\"가나\"}}]"),
            PackError::Capacity,
        ),
        (
            format!(
                ",\"tables\":[{{{}}}]",
                TABLE.replace("\"a\"", "\"c\"") + ",\"text\":\"a\""
            ),
            PackError::Key,
        ),
        (
            format!(
                ",\"tables\":[{{{}}}]",
                TABLE.replace("\"len\":3", "\"len\":0") + ",\"text\":\"a\""
            ),
            PackError::Key,
        ),
        (
            format!(
                ",\"tables\":[{{{}}}]",
                TABLE.replace("0123456789abcdef", "XYZ") + ",\"text\":\"a\""
            ),
            PackError::Hash,
        ),
        (
            format!(",\"tables\":[{{{TABLE},\"text\":\"a\\u0001\"}}]"),
            PackError::Text,
        ),
        (
            format!(",\"tables\":[{{{TABLE},\"text\":\"a\"}},{{{TABLE},\"text\":\"b\"}}]"),
            PackError::Duplicate,
        ),
        (
            ",\"strings\":[{\"hash\":\"0123456789abcdef\",\"len\":2,\"text\":\"가\"}]".into(),
            PackError::StringKey,
        ),
        (
            ",\"strings\":[{\"hash\":\"0123456789abcdef\",\"len\":257,\"text\":\"가\"}]".into(),
            PackError::StringKey,
        ),
        (
            format!(
                ",\"strings\":[{{\"hash\":\"0123456789abcdef\",\"len\":3,\"text\":\"{long}\"}}]"
            ),
            PackError::StringKey,
        ),
        (
            ",\"strings\":[{\"hash\":\"0123456789ABCDEF\",\"len\":3,\"text\":\"가\"}]".into(),
            PackError::Hash,
        ),
        (
            ",\"strings\":[{\"hash\":\"0123456789abcdef\",\"len\":3,\"text\":\"中\"}]".into(),
            PackError::Text,
        ),
        (
            ",\"strings\":[{\"hash\":\"0123456789abcdef\",\"len\":3,\"text\":\"가\"},{\"hash\":\"0123456789abcdef\",\"len\":4,\"text\":\"나\"}]".into(),
            PackError::Duplicate,
        ),
    ];
    for (i, (extra, want)) in cases.iter().enumerate() {
        assert_eq!(
            Pack::parse(&file(&json_with(extra))),
            Err(*want),
            "case {i}"
        );
    }
    for e in [PackError::StringKey, PackError::Capacity] {
        let text = e.to_string();
        assert!(!text.is_empty() && !text.contains(SENTINEL));
    }
    let marked = file(&json_with(&format!(
        ",\"strings\":[{{\"hash\":\"0123456789abcdef\",\"len\":1,\"text\":\"{SENTINEL}\"}}]"
    )));
    assert!(
        !Pack::parse(&marked)
            .unwrap_err()
            .to_string()
            .contains(SENTINEL)
    );
    // The builders hold the same gate as the reader: a text over its capacity is refused.
    let s = script();
    let over = Pack::new(Language::Korean, "t/1", 0, [])
        .unwrap()
        .with_tables([((b'a', 3, RED), table("빨강빨강", &s, RED, 3, Some(6)))]);
    assert_eq!(over, Err(PackError::Capacity));
    let fits = Pack::new(Language::Korean, "t/1", 0, [])
        .unwrap()
        .with_tables([((b'a', 3, RED), table("빨강", &s, RED, 3, Some(6)))]);
    assert!(fits.is_ok());
}

#[test]
fn table_strings_are_checked_with_the_runs_against_the_files() {
    let s = script();
    let p = pack(&s);
    let lib = archive(&s);
    assert_eq!(p.check(|l| (l == b'a').then_some(lib.as_slice())), Ok(()));
    let bad = Err(Mismatch {
        part: Part::Converse,
    });
    let base = fingerprint([(b'a', 3, hash64(&s))]);
    let with = |key: RunKey, t: TableText| {
        Pack::new(Language::Korean, "test/1", base, [])
            .unwrap()
            .with_tables([(key, t)])
            .unwrap()
            .check(|_| Some(lib.as_slice()))
    };
    // A script named only by a table string joins the fingerprint.
    assert_eq!(with((b'a', 3, RED), table("빨", &s, RED, 3, None)), Ok(()));
    let unnamed = Pack::new(Language::Korean, "test/1", fingerprint([]), [])
        .unwrap()
        .with_tables([((b'a', 3, RED), table("빨", &s, RED, 3, None))])
        .unwrap();
    assert_eq!(unnamed.check(|_| Some(lib.as_slice())), bad);
    // Not up to a NUL (a prefix of the string), past the script, another hash.
    assert_eq!(with((b'a', 3, RED), table("빨", &s, RED, 2, None)), bad);
    assert_eq!(
        with(
            (b'a', 3, BLUE),
            TableText {
                len: 40,
                hash: 0,
                capacity: None,
                text: b"x".to_vec()
            }
        ),
        bad
    );
    assert_eq!(
        with(
            (b'a', 3, RED),
            TableText {
                hash: 1,
                ..table("빨", &s, RED, 3, None)
            }
        ),
        bad
    );
    // The NUL between them is no string.
    assert_eq!(
        with((b'a', 3, RED + 3), table("빨", &s, RED + 3, 1, None)),
        bad
    );
}

#[test]
fn the_directives_are_read_as_printf_reads_them() {
    let d = |f: &[u8]| -> Vec<String> {
        directives(f)
            .into_iter()
            .map(|d| String::from_utf8(d).unwrap())
            .collect()
    };
    assert_eq!(
        d(b"Hi %s, %-5d%% %03x%>"),
        ["%s", "%-5d", "%%", "%03x", "%>"]
    );
    assert_eq!(d(b"%2$s then %1$s"), ["%2$s", "%1$s"]);
    assert_eq!(d(b"%0$s %12d"), ["%0$", "%12d"], "`0$` is no position");
    assert_eq!(d(b"end %"), ["%"]);
    assert_eq!(d(b"a%s\0%d"), ["%s"], "a NUL ends the format");
    assert!(d("가나다".as_bytes()).is_empty());
}

#[test]
fn a_game_string_whose_directives_differ_from_its_english_refuses_the_pack() {
    let cat = Catalogue::from_entries([
        (Kind::Format, b"You see %s.\n".to_vec()),
        (Kind::Message, b"Naught here".to_vec()),
    ]);
    let s = script();
    assert_eq!(pack(&s).check_strings(&cat), Ok(()));
    let refused = Err(Mismatch {
        part: Part::Strings,
    });
    for ko in [
        "그대는 보오.\n",
        "그대는 %d{을/를} 보오.\n",
        "%s %s\n",
        "그대는 %1$s{을/를} 보오.\n",
        "%%그대는 %s 보오.\n",
    ] {
        let p = Pack::new(Language::Korean, "t/1", 0, [])
            .unwrap()
            .with_strings([string("You see %s.\n", ko)])
            .unwrap();
        assert_eq!(p.check_strings(&cat), refused, "{ko}");
    }
    // A message has none, and its translation may hold none either.
    let p = Pack::new(Language::Korean, "t/1", 0, [])
        .unwrap()
        .with_strings([string("Naught here", "아무 %s")])
        .unwrap();
    assert_eq!(p.check_strings(&cat), refused);
    // A string the catalogue does not have is never looked up, and not checked.
    let p = Pack::new(Language::Korean, "t/1", 0, [])
        .unwrap()
        .with_strings([string("Other %d", "다른")])
        .unwrap();
    assert_eq!(p.check_strings(&cat), Ok(()));
    let text = Mismatch {
        part: Part::Strings,
    }
    .to_string();
    assert!(text.contains("game strings") && !text.contains("You see"));
}

#[test]
fn the_catalogue_keeps_each_string_once_with_its_kind_and_none_under_three_bytes() {
    let cat = Catalogue::from_entries([
        (Kind::Message, b"ab".to_vec()),
        (Kind::Message, b"abc".to_vec()),
        (Kind::Format, b"%d of %s".to_vec()),
        (Kind::Label, b"abc".to_vec()),
        (Kind::Message, Vec::new()),
        (Kind::Message, vec![b'x'; strings::MAX_BYTES + 1]),
        (Kind::Format, b"%s\n".to_vec()),
        (Kind::Format, b"\n%s:\n%>".to_vec()),
        (Kind::Format, b"%d stone/s $1 #2".to_vec()),
        (Kind::Format, b"#0/s $1 ^c\\es %-2d".to_vec()),
        (Kind::Message, b"123".to_vec()),
    ]);
    assert_eq!(
        cat.len(),
        3,
        "one each; nothing short, long, or without a word of its own"
    );
    assert!(cat.of(b"%d stone/s $1 #2").is_some());
    assert!(cat.of(b"ab").is_none());
    assert_eq!(
        cat.of(b"abc").map(|e| e.kind),
        Some(Kind::Label),
        "a label's kind wins"
    );
    let json = cat.to_json();
    let back: strings::Export = serde_json::from_str(&json).unwrap();
    assert_eq!(back.format, strings::FORMAT);
    assert_eq!(back.strings.len(), 3);
    assert_eq!(back.strings[1].text, "%d of %s");
    assert_eq!(back.strings[1].kind, Kind::Format);
    assert_eq!(back.strings[1].hash, hash64(b"%d of %s"));
    assert!(json.contains("\"kind\":\"label\"") && json.contains("\"len\":8"));
    // A file that is no hunk file catalogues nothing.
    assert!(Catalogue::from_game(b"not a game").is_empty());
    assert!(strings::reads("U6/GAME") && !strings::reads("U6/game.bak"));
}

#[test]
fn the_export_reader_refuses_with_fixed_sentences_that_name_nothing_of_the_input() {
    const FIELD: &str = "Zq8FieldSentinel";
    const KIND: &str = "Zq8KindSentinel";
    let ok = Catalogue::from_entries([(Kind::Message, b"Naught here".to_vec())]).to_json();
    assert!(strings::Export::from_json(&ok).is_ok());
    let unknown = ok.replacen('{', &format!("{{\"{FIELD}\":1,"), 1);
    let kind = ok.replace("\"message\"", &format!("\"{KIND}\""));
    let format = ok.replace(strings::FORMAT, &format!("{FIELD}/1"));
    let text = ok.replace("Naught here", &format!("Naught {FIELD}"));
    for (input, why) in [
        (unknown, "it is not JSON of the game strings' form"),
        (kind, "it is not JSON of the game strings' form"),
        (
            format!("[{KIND}"),
            "it is not JSON of the game strings' form",
        ),
        (
            format,
            "it is not of the game strings' format (u6.string-bundle/1)",
        ),
        (text, "a string's text, length and hash disagree"),
    ] {
        let err = strings::Export::from_json(&input).unwrap_err();
        assert_eq!(err, why);
        assert!(!err.contains(FIELD) && !err.contains(KIND));
    }
}

#[test]
fn a_pack_with_a_member_twice_is_refused_as_the_tools_reader_refuses_it() {
    // The engine's reader (serde) refuses a field given twice, at the root and inside an entry;
    // the tool's reader matches it (tools/i18n/strings.test.mjs).
    let twice = [
        json_with(",\"converse\":[]"),
        json_with(",\"strings\":[],\"strings\":[]"),
        json_with(&format!(
            ",\"tables\":[{{{TABLE},\"text\":\"a\",\"text\":\"b\"}}]"
        )),
        json_with("").replace(
            "\"base\":{\"converse\":",
            "\"base\":{\"converse\":\"0000000000000000\",\"converse\":",
        ),
    ];
    for (i, json) in twice.iter().enumerate() {
        assert_eq!(Pack::parse(&file(json)), Err(PackError::Shape), "case {i}");
    }
}

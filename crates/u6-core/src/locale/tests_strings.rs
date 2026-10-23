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
            ",\"strings\":[{\"hash\":\"0123456789abcdef\",\"len\":0,\"text\":\"가\"}]".into(),
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
        ",\"strings\":[{{\"hash\":\"0123456789abcdef\",\"len\":0,\"text\":\"{SENTINEL}\"}}]"
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
        (Kind::Format, b"zq".to_vec()),
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
    assert!(
        cat.label_of(b"ab").is_none() && cat.label_of(b"zq").is_none(),
        "a short message or format is not catalogued"
    );
    assert_eq!(
        cat.label_of(b"abc").map(|e| e.kind),
        cat.of(b"abc").map(|e| e.kind),
        "from three bytes, a label read finds what a print finds"
    );
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
    // A label of a pointer table is catalogued from 1 byte (2026-10-21), found only by a label
    // read, and exported with the others; a string of no word of its own still is not.
    let short = Catalogue::from_entries([
        (Kind::Label, b"Zq".to_vec()),
        (Kind::Label, b"Z".to_vec()),
        (Kind::Label, b"%s".to_vec()),
        (Kind::Label, Vec::new()),
        (Kind::Format, b"Zq".to_vec()),
    ]);
    assert_eq!(short.len(), 2);
    for zq in [&b"Zq"[..], b"Z"] {
        assert_eq!(short.label_of(zq).map(|e| e.kind), Some(Kind::Label));
        assert!(short.of(zq).is_none(), "a print never finds a short label");
    }
    assert!(short.label_of(b"").is_none());
    let json = short.to_json();
    assert!(json.contains("\"len\":2,\"kind\":\"label\",\"text\":\"Zq\""));
    let back = strings::Export::from_json(&json).unwrap();
    assert_eq!(back.strings.len(), 2);
    assert_eq!(
        (back.strings[0].len, back.strings[0].kind),
        (2, Kind::Label)
    );
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

/// The date line's format (A4-0x7293) in the synthetic `game`: the test's own.
const DATE: u32 = crate::image::A4 - 0x7293;
const DATE_FORMAT: &[u8] = b"Zq8 %d/%d/%d %s";
/// Where the synthetic `game` keeps the wind's names (past every string of the sites).
const WIND_TEXT: u32 = 0x0015_4800;

/// The strings the actors' and objects' own lines print as their format, by their sites' lists.
fn actor_and_object_sites() -> Vec<u32> {
    let lists: [&[u32]; 5] = [
        crate::actors::clock::PRINTED,
        crate::actors::turn::PRINTED,
        crate::actors::party::PRINTED,
        crate::actors::npc::PRINTED,
        crate::actors::walk::PRINTED,
    ];
    lists
        .into_iter()
        .chain(crate::objects::PRINTED)
        .flatten()
        .copied()
        .collect()
}

/// Each site's address with its synthetic text.
type SiteLines = Vec<(u32, Vec<u8>)>;

/// The synthetic text at each site (`Zq` and two digits: the sites lie 6 bytes apart at least),
/// the date line's format at its own, and a name at each pointer of the wind's name table (the
/// wind byte -1..=7): `Zw` and the entry's index, but two bytes for wind 7 (under
/// [`strings::MIN_BYTES`], as every wind's name in the original is).
fn site_texts() -> (SiteLines, Vec<Vec<u8>>) {
    let lines = actor_and_object_sites()
        .into_iter()
        .enumerate()
        .map(|(i, at)| {
            let text = if at == DATE {
                DATE_FORMAT.to_vec()
            } else {
                format!("Zq{i:02}").into_bytes()
            };
            (at, text)
        })
        .collect();
    let winds = (0..crate::actors::clock::LABELS.len())
        .map(|k| {
            if k == 8 {
                b"Zw".to_vec()
            } else {
                format!("Zw{k}").into_bytes()
            }
        })
        .collect();
    (lines, winds)
}

/// A synthetic `game` (one CODE hunk from the image base) holding [`site_texts`]: each string
/// NUL-ended at its site, the name table's pointers (absolute: no RELOC32) to the names.
fn site_game() -> Vec<u8> {
    let (lines, winds) = site_texts();
    let base = crate::image::IMAGE_BASE;
    let mut hunk = vec![0u8; (WIND_TEXT - base) as usize + 0x100];
    let mut put = |at: u32, bytes: &[u8]| {
        let o = (at - base) as usize;
        hunk[o..o + bytes.len()].copy_from_slice(bytes);
    };
    for (at, text) in &lines {
        put(*at, &[text.as_slice(), &[0]].concat());
    }
    for (k, (&ptr, name)) in crate::actors::clock::LABELS.iter().zip(&winds).enumerate() {
        let at = WIND_TEXT + 8 * k as u32;
        put(ptr, &at.to_be_bytes());
        put(at, &[name.as_slice(), &[0]].concat());
    }
    crate::image::SyntheticGame::new()
        .loaded(false, hunk)
        .build()
}

#[test]
fn the_actors_and_objects_lines_and_the_wind_names_are_catalogued() {
    let cat = Catalogue::from_game(&site_game());
    let (lines, winds) = site_texts();
    assert_eq!(
        lines.len(),
        22,
        "the clock 3, the turn 4, the party 6, the NPCs 4, a step 1, the objects 4"
    );
    for (at, text) in &lines {
        let kind = if text == DATE_FORMAT {
            Kind::Format
        } else {
            Kind::Message
        };
        assert_eq!(
            cat.of(text).map(|e| e.kind),
            Some(kind),
            "the line at {at:#x}"
        );
    }
    for (k, name) in winds.iter().enumerate() {
        assert_eq!(
            cat.label_of(name).map(|e| e.kind),
            Some(Kind::Label),
            "wind name {k}"
        );
    }
    // A name under three bytes is catalogued as a label (2026-10-21), and a print never finds it.
    assert_eq!(cat.label_of(b"Zw").map(|e| e.text.len()), Some(2));
    assert!(cat.of(b"Zw").is_none());
    assert!(
        cat.export()
            .strings
            .iter()
            .any(|e| e.text == "Zw" && e.kind == Kind::Label),
        "the export lists it"
    );
}

/// The date line of wind `wind` printed in an engine over [`site_game`], with `pack` installed
/// over its catalogue (or no pack): the format queued and its arguments.
fn date_line(pack: Option<Pack>, wind: u8) -> (Vec<u8>, Vec<crate::main_loop::PrintArg>) {
    let game = site_game();
    let files = Files::new(BTreeMap::from([("game".to_string(), game.clone())]));
    let mut s = State::empty();
    s.actors.calendar.month = 4;
    s.actors.calendar.day = 9;
    s.actors.calendar.year = 161;
    s.actors.wind = wind;
    if let Some(p) = pack {
        install_catalogued(
            &mut s,
            &files,
            Rc::new(p),
            None,
            Catalogue::from_game(&game),
        )
        .expect("the pack checks");
    }
    let mut e = crate::engine::Engine::with_game(s, files, |st, _io| async move {
        crate::actors::clock::date_line(&mut st.borrow_mut());
    });
    for _ in 0..8 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let printed: Vec<_> = e
        .state()
        .main
        .pending_print
        .iter()
        .filter_map(|p| match p {
            crate::main_loop::Pending::Print(f, a) => Some((f.clone(), a.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(printed.len(), 1, "the date line is queued once");
    printed[0].clone()
}

#[test]
fn the_date_line_shows_the_packs_wind_name_and_without_a_pack_the_english() {
    use crate::main_loop::PrintArg::{Str, Word};
    let ko = || {
        Pack::new(Language::Korean, "test/1", fingerprint([]), [])
            .unwrap()
            .with_strings([string("Zw0", "잔잔"), string("Zw3", "높새")])
            .unwrap()
    };
    let words = vec![Word(4), Word(9), Word(161)];
    // Calm (the wind byte 0xff, the table's entry -1): the pack's name with a pack, the English
    // without one, the format and the date the same.
    let (format, args) = date_line(None, 0xff);
    assert_eq!(format, DATE_FORMAT);
    assert_eq!(args, [words.clone(), vec![Str(b"Zw0".to_vec())]].concat());
    let (format, args) = date_line(Some(ko()), 0xff);
    assert_eq!(
        format, DATE_FORMAT,
        "the format is the English (the pack has none for it)"
    );
    assert_eq!(
        args,
        [words.clone(), vec![Str("잔잔".as_bytes().to_vec())]].concat()
    );
    // Wind 2 likewise; a name the pack lacks (wind 5's) stays English.
    let (_, args) = date_line(Some(ko()), 2);
    assert_eq!(args[3], Str("높새".as_bytes().to_vec()));
    let (_, args) = date_line(Some(ko()), 5);
    assert_eq!(args[3], Str(b"Zw6".to_vec()));
}

/// A pack of the two-byte wind name (wind 7's, [`site_texts`]) and a three-byte one.
fn short_ko() -> Pack {
    Pack::new(Language::Korean, "test/1", fingerprint([]), [])
        .unwrap()
        .with_strings([string("Zw", "샛바람"), string("Zw0", "잔잔")])
        .unwrap()
}

#[test]
fn a_two_byte_wind_name_shows_the_packs_and_without_a_pack_the_english() {
    use crate::main_loop::PrintArg::{Str, Word};
    // The pack reads a two-byte English (it holds no kind) and keeps it.
    let p = short_ko();
    assert_eq!(Pack::parse(&p.to_bytes()), Ok(p.clone()));
    // Wind 7: the pack's name; the format the English (the pack has none for it).
    let (format, args) = date_line(Some(short_ko()), 7);
    assert_eq!(format, DATE_FORMAT);
    assert_eq!(
        args,
        [
            Word(4),
            Word(9),
            Word(161),
            Str("샛바람".as_bytes().to_vec())
        ]
    );
    // Without a pack the line is the English, as before.
    let (format, args) = date_line(None, 7);
    assert_eq!(format, DATE_FORMAT);
    assert_eq!(args, [Word(4), Word(9), Word(161), Str(b"Zw".to_vec())]);
}

/// What game code's prints put for `Zw` (`printf`'s format, `puts`, `put_raw_str`, a `|` between
/// them) in an engine over [`site_game`] with `pack` installed over its catalogue: the scroll's
/// text, the counts after them, and what [`shown`] reads for the same bytes after them.
fn printed_zw(pack: Pack) -> (String, [u32; 2], Vec<u8>) {
    let game = site_game();
    let files = Files::new(BTreeMap::from([("game".to_string(), game.clone())]));
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    install_catalogued(
        &mut s,
        &files,
        Rc::new(pack),
        None,
        Catalogue::from_game(&game),
    )
    .expect("the pack checks");
    let mut e = crate::engine::Engine::with_game(s, files, |st, io| async move {
        crate::ui::printf::printf(&st, &io, b"Zw", &[]).await;
        crate::ui::printf::puts(&st, &io, b"|").await;
        crate::ui::printf::puts(&st, &io, b"Zw").await;
        crate::ui::print::put_raw_str(&st, &io, b"|").await;
        crate::ui::print::put_raw_str(&st, &io, b"Zw").await;
        crate::ui::print::put_raw(&st, &io, -1).await;
    });
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let s = e.state();
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    let cells = t.transcript_line.iter().chain(pending);
    lines.push(cells.map(|&c| crate::ui::wide::cell_char(c)).collect());
    let counts = s.locale.texts.get();
    let label = shown(&s, b"Zw".to_vec());
    (lines.join("\n"), counts, label)
}

#[test]
fn a_print_of_the_two_bytes_of_a_short_label_stays_english() {
    let (screen, counts, label) = printed_zw(short_ko());
    assert_eq!(
        screen.trim_end(),
        "Zw|Zw|Zw",
        "printf's format, puts and put_raw_str of the same bytes: English"
    );
    assert_eq!(counts, [0, 0], "no print looked it up");
    assert_eq!(
        label,
        "샛바람".as_bytes(),
        "the label read of the same state finds it"
    );
}

/// Combat's sites the wound report (0x12395a) prints, the test's own copies of
/// `combat::damage`'s addresses (pinned to its list in the tests): the name-and-space format,
/// the critical line, the three degrees and the wounded word; and the roster's number line.
const NAME_SPACE: u32 = 0x0012_1b9a;
const HP_NUMBER: u32 = 0x0012_1b94;
const CRITICAL: u32 = crate::image::A4 - 0x7750;
const HEAVILY: u32 = crate::image::A4 - 0x7745;
const LIGHTLY: u32 = crate::image::A4 - 0x773d;
const BARELY: u32 = crate::image::A4 - 0x7735;
const WOUNDED: u32 = crate::image::A4 - 0x772e;

/// The strings combat prints as its format, by its sites' lists (a hit's, the refused cast's).
fn combat_sites() -> Vec<u32> {
    crate::combat::damage::PRINTED
        .iter()
        .chain(crate::combat::seams::PRINTED)
        .copied()
        .collect()
}

/// The synthetic text at each of [`combat_sites`] (`Zc` and two digits: the sites lie 6 bytes
/// apart at least), but two formats of no word of their own: a name and a space at the wound
/// report's first and a number line at the roster's.
fn combat_texts() -> SiteLines {
    combat_sites()
        .into_iter()
        .enumerate()
        .map(|(i, at)| {
            let text = match at {
                NAME_SPACE => b"%s ".to_vec(),
                HP_NUMBER => b"%d\n".to_vec(),
                _ => format!("Zc{i:02}").into_bytes(),
            };
            (at, text)
        })
        .collect()
}

/// The text [`combat_texts`] puts at `at`.
fn combat_text(at: u32) -> String {
    let (_, text) = combat_texts()
        .into_iter()
        .find(|&(a, _)| a == at)
        .expect("a combat site");
    String::from_utf8(text).unwrap()
}

/// A synthetic `game` (one CODE hunk from the image base) holding [`combat_texts`], each string
/// NUL-ended at its site.
fn combat_game() -> Vec<u8> {
    game_of(&combat_texts())
}

/// A synthetic `game` (one CODE hunk from the image base) holding `lines`, each string NUL-ended
/// at its site.
fn game_of(lines: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let base = crate::image::IMAGE_BASE;
    let end = lines
        .iter()
        .map(|(at, text)| (at - base) as usize + text.len() + 1)
        .max()
        .unwrap_or(0);
    let mut hunk = vec![0u8; end.next_multiple_of(4)];
    for (at, text) in lines {
        let o = (at - base) as usize;
        hunk[o..o + text.len()].copy_from_slice(text);
    }
    crate::image::SyntheticGame::new()
        .loaded(false, hunk)
        .build()
}

#[test]
fn combats_lines_are_catalogued_but_its_formats_of_no_word_of_their_own() {
    for at in [
        NAME_SPACE, HP_NUMBER, CRITICAL, HEAVILY, LIGHTLY, BARELY, WOUNDED,
    ] {
        assert!(
            crate::combat::damage::PRINTED.contains(&at),
            "{at:#x} is in a hit's list"
        );
    }
    // Both lists in full, as the test's own literals (a constant dropped, added twice or moved
    // fails here): the graze, the death line, dragged, dissolves, robbed, poisoned, the critical
    // line, the three degrees, the wounded word, the slime, the vine, broken, the yell, the name
    // and space and the number line; and the refusal.
    let a4 = crate::image::A4;
    let sorted = |list: &[u32]| {
        let mut v = list.to_vec();
        v.sort_unstable();
        v
    };
    assert_eq!(
        sorted(crate::combat::damage::PRINTED),
        sorted(&[
            a4 - 0x7723,
            a4 - 0x7716,
            a4 - 0x75c3,
            a4 - 0x75a0,
            a4 - 0x7585,
            a4 - 0x75af,
            a4 - 0x7750,
            a4 - 0x7745,
            a4 - 0x773d,
            a4 - 0x7735,
            a4 - 0x772e,
            a4 - 0x7760,
            a4 - 0x755a,
            a4 - 0x7361,
            a4 - 0x7318,
            0x0012_1b9a,
            0x0012_1b94,
        ]),
        "a hit's list"
    );
    assert_eq!(
        crate::combat::seams::PRINTED,
        [0x0011_59f8],
        "the refused cast's list"
    );
    let cat = Catalogue::from_game(&combat_game());
    let lines = combat_texts();
    assert_eq!(lines.len(), 18, "a hit's 17, the refused cast's 1");
    for (at, text) in &lines {
        let kind = cat.of(text).map(|e| e.kind);
        if [NAME_SPACE, HP_NUMBER].contains(at) {
            assert_eq!(kind, None, "the format at {at:#x} has no word of its own");
        } else {
            assert_eq!(kind, Some(Kind::Message), "the line at {at:#x}");
        }
    }
    assert_eq!(cat.len(), 16, "every line but the two formats");
}

/// The wound report (0x12395a) of party member `Zm` (actor 1, level 2: 60 hit points at most)
/// at `hp` hit points in an engine over [`combat_game`], with `pack` installed over its
/// catalogue (or no pack), and the lines it queued printed: the scroll's text and the counts.
fn wound_report(pack: Option<Pack>, hp: u8) -> (String, [u32; 2]) {
    let game = combat_game();
    let files = Files::new(BTreeMap::from([("game".to_string(), game.clone())]));
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    s.objects.type_frame[1] = 0x17a;
    s.actors.attrs.status[1] = 0x80;
    s.actors.attrs.level[1] = 2;
    s.actors.attrs.hp[1] = hp;
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.actors.party.names = vec!["Zm".into()];
    if let Some(p) = pack {
        install_catalogued(
            &mut s,
            &files,
            Rc::new(p),
            None,
            Catalogue::from_game(&game),
        )
        .expect("the pack checks");
    }
    let mut e = crate::engine::Engine::with_game(s, files, |st, io| async move {
        crate::combat::damage::report_wounds(&mut st.borrow_mut(), 1);
        crate::main_loop::flush_prints(&st, &io).await;
    });
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let s = e.state();
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    let cells = t.transcript_line.iter().chain(pending);
    lines.push(cells.map(|&c| crate::ui::wide::cell_char(c)).collect());
    (lines.join("\n"), s.locale.texts.get())
}

#[test]
fn the_wound_report_prints_the_packs_forms_in_order_and_without_a_pack_the_english() {
    let ko = || {
        Pack::new(Language::Korean, "test/1", fingerprint([]), [])
            .unwrap()
            .with_strings([
                string(&combat_text(CRITICAL), "빈사"),
                string(&combat_text(HEAVILY), "중상"),
                string(&combat_text(LIGHTLY), "경상"),
                string(&combat_text(BARELY), "찰과상"),
                string(&combat_text(WOUNDED), "을 입음"),
            ])
            .unwrap()
    };
    let (heavily, wounded) = (combat_text(HEAVILY), combat_text(WOUNDED));
    // A quarter of its hit points: the name and a space (no word: as it is), then the pack's
    // degree and its wounded word, in that order, both looked up and found.
    let (screen, counts) = wound_report(Some(ko()), 15);
    assert_eq!(screen.trim_end(), "Zm 중상을 입음");
    assert_eq!(
        counts,
        [2, 0],
        "the degree and the wounded word from the pack"
    );
    // Without a pack the English, nothing counted.
    let (screen, counts) = wound_report(None, 15);
    assert_eq!(screen.trim_end(), format!("Zm {heavily}{wounded}"));
    assert_eq!(counts, [0, 0]);
    // The other quarters likewise: the critical line alone, the lighter degrees with the word.
    for (hp, ko_line, en_line) in [
        (14, "Zm 빈사", format!("Zm {}", combat_text(CRITICAL))),
        (
            30,
            "Zm 경상을 입음",
            format!("Zm {}{wounded}", combat_text(LIGHTLY)),
        ),
        (
            45,
            "Zm 찰과상을 입음",
            format!("Zm {}{wounded}", combat_text(BARELY)),
        ),
    ] {
        assert_eq!(
            wound_report(Some(ko()), hp).0.trim_end(),
            ko_line,
            "hp {hp}"
        );
        assert_eq!(wound_report(None, hp).0.trim_end(), en_line, "hp {hp}");
    }
    // Unhurt: no report.
    assert_eq!(wound_report(Some(ko()), 60), (String::new(), [0, 0]));
}

/// The two combat lines whose English starts with a newline, the test's own copies of their
/// addresses (pinned to their sites' lists in the test): the refusal of a staff's or a wand's
/// cast (`combat::seams`) and the next weapon's prompt (`combat::attack`).
const CAST_REFUSED: u32 = 0x0011_59f8;
const NEXT_WEAPON: u32 = crate::image::A4 - 0x76ac;

/// A pack of a form for each line of [`after_a_held_newline`]'s `game`, the newline kept.
fn held_ko() -> Pack {
    Pack::new(Language::Korean, "test/1", fingerprint([]), [])
        .unwrap()
        .with_strings([string("\nZr1", "\n거부"), string("\nZn %s", "\n%s 차례")])
        .unwrap()
}

/// In an engine over a synthetic `game` of the two lines (`\nZr1`, `\nZn %s`), with `pack`
/// installed over its catalogue (or no pack): `Zp` printed (no newline after it), Attack's
/// newline held (`combat.swallow`) when `held`, then `line` run and the lines it queued printed:
/// the scroll's text, the counts, and whether the newline is still held.
fn after_a_held_newline(
    pack: Option<Pack>,
    held: bool,
    line: fn(&mut State),
) -> (String, [u32; 2], bool) {
    let game = game_of(&[
        (CAST_REFUSED, b"\nZr1".to_vec()),
        (NEXT_WEAPON, b"\nZn %s".to_vec()),
    ]);
    let files = Files::new(BTreeMap::from([("game".to_string(), game.clone())]));
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    if let Some(p) = pack {
        install_catalogued(
            &mut s,
            &files,
            Rc::new(p),
            None,
            Catalogue::from_game(&game),
        )
        .expect("the pack checks");
    }
    let mut e = crate::engine::Engine::with_game(s, files, move |st, io| async move {
        crate::ui::printf::printf(&st, &io, b"Zp", &[]).await;
        {
            let mut s = st.borrow_mut();
            s.combat.swallow = held;
            line(&mut s);
        }
        crate::main_loop::flush_prints(&st, &io).await;
    });
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let s = e.state();
    (
        scroll(&s).trim_end().to_string(),
        s.locale.texts.get(),
        s.combat.swallow,
    )
}

/// The scroll's text: the lines so far, the one being written last.
fn scroll(s: &State) -> String {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    let cells = t.transcript_line.iter().chain(pending);
    lines.push(cells.map(|&c| crate::ui::wide::cell_char(c)).collect());
    lines.join("\n")
}

#[test]
fn a_line_after_attacks_held_newline_shows_the_packs_form_without_its_newline() {
    use crate::combat::present::Text;
    assert!(crate::combat::seams::PRINTED.contains(&CAST_REFUSED));
    assert!(crate::combat::attack::PRINTED.contains(&NEXT_WEAPON));
    let refused: fn(&mut State) = crate::combat::seams::cast_from_item;
    let next: fn(&mut State) = |s| {
        crate::combat::present::say(s, NEXT_WEAPON, vec![Text::Str(b"Zb".to_vec())]);
    };
    // Attack's newline held: each line is looked up whole, its pack form put without its
    // newline, and the newline is no longer held.
    assert_eq!(
        after_a_held_newline(Some(held_ko()), true, refused),
        ("Zp거부".to_string(), [1, 0], false),
        "the refusal"
    );
    assert_eq!(
        after_a_held_newline(Some(held_ko()), true, next),
        ("ZpZb 차례".to_string(), [1, 0], false),
        "the next weapon's prompt"
    );
    // Without a pack the English without its newline, as before.
    assert_eq!(
        after_a_held_newline(None, true, refused),
        ("ZpZr1".to_string(), [0, 0], false)
    );
    assert_eq!(
        after_a_held_newline(None, true, next),
        ("ZpZn Zb".to_string(), [0, 0], false)
    );
    // No newline held: the pack's form with its newline.
    assert_eq!(
        after_a_held_newline(Some(held_ko()), false, refused),
        ("Zp\n거부".to_string(), [1, 0], false)
    );
}

/// Where [`said_after_a_held_newline`]'s synthetic `game` keeps its lines (the n-th 0x20 bytes
/// after this; no catalogued site).
const HELD_LINES: u32 = crate::image::IMAGE_BASE + 0x100;

/// In an engine over a synthetic `game` of `lines` (no pack): `Zp` printed (no newline after
/// it), Attack's newline held (`combat.swallow`), then each line said in turn: through
/// [`crate::combat::present::say`], or (`old`) as `say` queued it before 2026-10-24, the leading
/// newlines drained from the English at the queue and the newline held while nothing was left;
/// the queue printed after each. The scroll's text (not trimmed) and whether the newline is held
/// after each line.
fn said_after_a_held_newline(lines: &[&[u8]], old: bool) -> (String, Vec<bool>) {
    let at = |i: usize| HELD_LINES + 0x20 * i as u32;
    let sites: Vec<(u32, Vec<u8>)> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| (at(i), l.to_vec()))
        .collect();
    let files = Files::new(BTreeMap::from([("game".to_string(), game_of(&sites))]));
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    let held = Rc::new(std::cell::RefCell::new(Vec::new()));
    let out = Rc::clone(&held);
    let mut e = crate::engine::Engine::with_game(s, files, move |st, io| async move {
        crate::ui::printf::printf(&st, &io, b"Zp", &[]).await;
        st.borrow_mut().combat.swallow = true;
        for (i, english) in sites.iter().map(|(_, l)| l).enumerate() {
            {
                let mut s = st.borrow_mut();
                if old {
                    let mut fmt = english.clone();
                    if s.combat.swallow {
                        let n = fmt.iter().take_while(|&&b| b == b'\n').count();
                        fmt.drain(..n);
                        if !fmt.is_empty() {
                            s.combat.swallow = false;
                        }
                    }
                    crate::main_loop::queue_print(&mut s, fmt, Vec::new());
                } else {
                    crate::combat::present::say(&mut s, at(i), Vec::new());
                }
                out.borrow_mut().push(s.combat.swallow);
            }
            crate::main_loop::flush_prints(&st, &io).await;
        }
    });
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let screen = scroll(&e.state());
    (screen, held.borrow().clone())
}

/// One case of the held newline: its name, the lines said in turn, the scroll after them and
/// whether the newline is held after each.
type HeldCase<'a> = (&'a str, &'a [&'a [u8]], &'a str, &'a [bool]);

#[test]
fn a_line_said_after_a_held_newline_shows_and_holds_as_the_old_drain_did() {
    let cases: [HeldCase; 4] = [
        ("an empty format", &[b""], "Zp", &[true]),
        ("a newline only", &[b"\n"], "Zp", &[true]),
        ("two newlines, then text", &[b"\n\nZt"], "ZpZt", &[false]),
        (
            "a newline only, then text",
            &[b"\n", b"Zs"],
            "ZpZs",
            &[true, false],
        ),
    ];
    for (case, lines, screen, held) in cases {
        let now = said_after_a_held_newline(lines, false);
        assert_eq!(
            now,
            said_after_a_held_newline(lines, true),
            "{case}: as before"
        );
        assert_eq!(now, (screen.to_string(), held.to_vec()), "{case}");
    }
}

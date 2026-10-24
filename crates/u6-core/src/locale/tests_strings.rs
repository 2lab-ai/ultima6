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

/// The inventory's sites, the test's own copies of the addresses (pinned to their lists in the
/// test): Ready's (0x112052) line, a torch's line, the name line, the three refusals, the
/// prompt, the line format, the newline and Unready's (0x1123d6) line; the burned-out torch's
/// line; the locked chest's line and the prompt of the click (0x10619c, `ui::click`).
const NAME_LINE: u32 = 0x0011_0baa;
const READY: u32 = 0x0011_0c14;
const TORCH_LINE: u32 = 0x0011_0c1c;
const CANT: u32 = 0x0011_0c24;
const HEAVY: u32 = 0x0011_0c38;
const NO_PLACE: u32 = 0x0011_0c46;
const PROMPT: u32 = 0x0011_0c5a;
const LINE: u32 = 0x0011_0c62;
const NEWLINE: u32 = 0x0011_0c68;
const UNREADY: u32 = 0x0011_0c6a;
const BURNED_OUT: u32 = crate::image::A4 - 0x756f;
const LOCKED: u32 = 0x0010_4fe6;
const CLICK_PROMPT: u32 = 0x0010_4ff4;
/// The pointer of the amulet's line Unready shows through `%s` (A4-0x6c3c, entry 12 of the
/// shared messages' table), and where the synthetic `game` keeps the line (no catalogued site).
const AMULET_POINTER: u32 = crate::image::A4 - 0x6c3c;
const AMULET_TEXT: u32 = crate::image::IMAGE_BASE + 0x200;

/// The synthetic text at each inventory site and the amulet's line (the test's own; each fits
/// before the next site with its NUL: the newline's site lies 2 bytes before Unready's): a line
/// at each site that prints one, but four formats of no word of their own (the name line, the
/// two prompts, the line format) and the newline.
fn inventory_texts() -> SiteLines {
    vec![
        (NAME_LINE, b"%s\n".to_vec()),
        (READY, b"Zr1 ".to_vec()),
        (TORCH_LINE, b"Zt1\n".to_vec()),
        (CANT, b"Zc1\n".to_vec()),
        (HEAVY, b"Zh1\n".to_vec()),
        (NO_PLACE, b"Zp1\n".to_vec()),
        (PROMPT, b"\n=%s\n".to_vec()),
        (LINE, b"%s\n".to_vec()),
        (NEWLINE, b"\n".to_vec()),
        (UNREADY, b"Zu1 %s\n".to_vec()),
        (BURNED_OUT, b"Zb1\n".to_vec()),
        (LOCKED, b"Zl1\n".to_vec()),
        (CLICK_PROMPT, b"\n=%s\n".to_vec()),
        (AMULET_TEXT, b"Za1".to_vec()),
    ]
}

/// A synthetic `game` (one CODE hunk from the image base) holding [`inventory_texts`], each
/// NUL-ended at its site, and the amulet's pointer (absolute: no RELOC32) to its line.
fn inventory_game() -> Vec<u8> {
    let mut lines = inventory_texts();
    lines.push((AMULET_POINTER, AMULET_TEXT.to_be_bytes().to_vec()));
    game_of(&lines)
}

#[test]
fn the_inventorys_lines_are_catalogued_but_its_formats_of_no_word_of_their_own() {
    // The lists in full, as the test's own literals (a constant dropped, added twice or moved
    // fails here).
    let sorted = |list: &[u32]| {
        let mut v = list.to_vec();
        v.sort_unstable();
        v
    };
    assert_eq!(
        sorted(crate::inventory::PRINTED),
        sorted(&[
            READY, TORCH_LINE, NAME_LINE, CANT, HEAVY, NO_PLACE, PROMPT, LINE, NEWLINE, UNREADY,
            BURNED_OUT,
        ]),
        "Ready's and Unready's list"
    );
    assert_eq!(
        crate::ui::click::PRINTED,
        [LOCKED, CLICK_PROMPT],
        "the click's list"
    );
    assert_eq!(
        crate::inventory::LABELS,
        [AMULET_POINTER],
        "Unready's label"
    );
    let cat = Catalogue::from_game(&inventory_game());
    for (at, text) in inventory_texts() {
        let want = match at {
            NAME_LINE | PROMPT | LINE | NEWLINE | CLICK_PROMPT => None,
            UNREADY => Some(Kind::Format),
            AMULET_TEXT => Some(Kind::Label),
            _ => Some(Kind::Message),
        };
        assert_eq!(cat.of(&text).map(|e| e.kind), want, "the string at {at:#x}");
    }
    assert_eq!(
        cat.len(),
        9,
        "seven lines, Unready's format and the amulet's label"
    );
}

/// Object types of [`holding`]'s world (tile = 4 × type): one that readies nowhere, two helms
/// (slot 0; the first too heavy for the actor), the torch and the amulet of Unready's checks.
const NO_SLOT: u16 = 0x30;
const HEAVY_HELM: u16 = 0x80;
const HELM: u16 = 0x81;
const TORCH: u16 = 0x5a;
const AMULET: u16 = 0x4c;

/// Party member `Zm` (actor 1, strength 20) holding an object of type and frame `tf` placed by
/// `status` (in its inventory or readied), selected in the inventory panel of member 0: tiles
/// 4 × type + frame, [`HEAVY_HELM`] weighing 0xff, no look table (every object is named `zo`,
/// the nothing word).
fn holding(tf: u16, status: u8) -> State {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    s.tables.basetile.base = (0..1024).map(|t| t * 4).collect();
    s.tables.tileflags.weight = vec![0; 1024];
    s.tables.tileflags.weight[usize::from(HEAVY_HELM)] = 0xff;
    s.objects.defs.nothing_lower = "zo".into();
    crate::objects::init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    let p = &mut s.actors.party;
    p.size = 1;
    p.members[0] = 1;
    p.names = vec!["Zm".into()];
    s.actors.attrs.strength[1] = 20;
    s.actors.attrs.status[1] = 0x80;
    s.objects.type_frame[1] = 0x178;
    let obj = crate::objects::create_in(&mut s, tf, status, 1, 1);
    assert!(obj > 0, "a slot for the object");
    s.ui.input.selection = obj;
    s
}

/// A pack of a form for each line Ready and Unready print and the amulet's (the test's own).
fn inventory_ko() -> Pack {
    Pack::new(Language::Korean, "test/1", fingerprint([]), [])
        .unwrap()
        .with_strings([
            string("Zr1 ", "가 "),
            string("Zt1\n", "나\n"),
            string("Zc1\n", "다\n"),
            string("Zh1\n", "라\n"),
            string("Zp1\n", "마\n"),
            string("Zu1 %s\n", "바 %s\n"),
            string("Zb1\n", "사\n"),
            string("Za1", "아"),
        ])
        .unwrap()
}

/// Ready (or, `unready`, Unready) of the selected object of `s` in an engine over
/// [`inventory_game`], with `pack` installed over its catalogue (or no pack): the scroll's text
/// and the counts.
fn inventory_command(mut s: State, pack: Option<Pack>, unready: bool) -> (String, [u32; 2]) {
    let game = inventory_game();
    let files = Files::new(BTreeMap::from([("game".to_string(), game.clone())]));
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
        if unready {
            crate::inventory::unready(&st, &io).await;
        } else {
            crate::inventory::ready(&st, &io).await;
        }
    });
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let s = e.state();
    (scroll(&s).trim_end().to_string(), s.locale.texts.get())
}

/// One case of Ready or Unready: its name, the object's type and frame and its place, whether
/// Unready runs, what else the actor's equipment holds, then the scroll with the pack and the
/// strings it found there, and the scroll without a pack.
type InventoryCase<'a> = (
    &'a str,
    u16,
    u8,
    bool,
    fn(&mut State),
    &'a str,
    u32,
    &'a str,
);

#[test]
fn ready_and_unready_print_the_packs_forms_in_order_and_without_a_pack_the_english() {
    use crate::objects::{IN_INVENTORY, READIED};
    let nothing: fn(&mut State) = |_| {};
    let worn: fn(&mut State) = |s| {
        let obj = s.ui.input.selection;
        s.inventory.set_equip(0, obj);
    };
    let helm_worn: fn(&mut State) = |s| {
        let other = crate::objects::create_in(s, HELM, READIED, 1, 1);
        s.inventory.set_equip(0, other);
    };
    let cases: [InventoryCase; 7] = [
        // Ready refused: the Ready line, the name line (no word: as it is), the refusal, then
        // the prompt (no word); the pack's forms in that order, each looked up and found.
        (
            "no slot",
            NO_SLOT,
            IN_INVENTORY,
            false,
            nothing,
            "가 zo\n다\n\n=Zm",
            2,
            "Zr1 zo\nZc1\n\n=Zm",
        ),
        (
            "too heavy",
            HEAVY_HELM,
            IN_INVENTORY,
            false,
            nothing,
            "가 zo\n라\n\n=Zm",
            2,
            "Zr1 zo\nZh1\n\n=Zm",
        ),
        (
            "the slot taken",
            HELM,
            IN_INVENTORY,
            false,
            helm_worn,
            "가 zo\n마\n\n=Zm",
            2,
            "Zr1 zo\nZp1\n\n=Zm",
        ),
        // A torch prints its own line in place of the name.
        (
            "a torch",
            TORCH,
            IN_INVENTORY,
            false,
            nothing,
            "가 나\n다\n\n=Zm",
            3,
            "Zr1 Zt1\nZc1\n\n=Zm",
        ),
        // Unready: its line with the name, then the prompt.
        (
            "unready",
            HELM,
            READIED,
            true,
            worn,
            "바 zo\n\n=Zm",
            1,
            "Zu1 zo\n\n=Zm",
        ),
        // The amulet stays: its line, a label read for display, through the line format.
        (
            "the amulet",
            AMULET,
            READIED,
            true,
            worn,
            "바 zo\n아\n\n=Zm",
            2,
            "Zu1 zo\nZa1\n\n=Zm",
        ),
        // A lit torch burns out: a newline, then its line.
        (
            "a lit torch",
            TORCH | (1 << 10),
            READIED,
            true,
            worn,
            "바 zo\n\n사\n\n=Zm",
            2,
            "Zu1 zo\n\nZb1\n\n=Zm",
        ),
    ];
    for (case, tf, status, unready, setup, ko, found, en) in cases {
        let world = || {
            let mut s = holding(tf, status);
            setup(&mut s);
            s
        };
        assert_eq!(
            inventory_command(world(), Some(inventory_ko()), unready),
            (ko.to_string(), [found, 0]),
            "{case}: the pack's forms"
        );
        assert_eq!(
            inventory_command(world(), None, unready),
            (en.to_string(), [0, 0]),
            "{case}: the English"
        );
    }
}

/// Magic's sites the tests below print or sort out, the test's own copies of the addresses
/// (pinned to their lists in the test): the result formats (`%s`, `%s\n`, `\n%s\n`, `\n%s!\n`)
/// and the dispatcher's newline, the book's number and the announcement's formats, the
/// direction's and Locate's formats, the newline after a typed spell's search (hunk 2); the
/// closing quote and Locate's hemisphere letters; the charge, potion and food lines; Cast's
/// no-book line, the books' no-magic and no-spells lines and the typed spell's not-in-book line
/// (A4-relative).
const SPELL_FMT_S: u32 = 0x0011_58b0;
const SPELL_NEWLINE: u32 = 0x0011_58ba;
const SPELL_FMT_S_NL: u32 = 0x0011_58ca;
const SPELL_FMT_NL_S_NL: u32 = 0x0011_58ce;
const SPELL_FMT_NL_S_BANG: u32 = 0x0011_58e4;
const BOOK_COUNT: u32 = 0x0011_58ac;
const SPELL_FMT_SPACE_S: u32 = 0x0011_5a62;
const ANNOUNCE_TYPED: u32 = 0x0011_5ada;
const ANNOUNCE_NAME: u32 = 0x0011_5ae0;
const DIRECTION_NAME: u32 = 0x0011_5a94;
const LATITUDE: u32 = 0x0011_5a72;
const LONGITUDE: u32 = 0x0011_5a7c;
const SEARCH_NEWLINE: u32 = 0x0010_4ffc;
const CLOSING_QUOTE: u32 = 0x0011_5ae6;
const SOUTH: u32 = 0x0011_5a78;
const NORTH: u32 = 0x0011_5a7a;
const EAST: u32 = 0x0011_5a82;
const WEST: u32 = 0x0011_5a86;
const CHARGE_LINE: u32 = 0x0011_5978;
const POTION_LINE: u32 = 0x0011_59a8;
const FOOD_LINE: u32 = 0x0011_59ca;
const NO_BOOK: u32 = 0x0011_5b36;
const NO_MAGIC: u32 = 0x0011_5a9a;
const NO_SPELLS: u32 = 0x0011_5ab2;
const NOT_IN_BOOK: u32 = crate::image::A4 - 0x7387;
/// Wind Change's direction prompt (Telekinesis' too).
const DIRECTION_PROMPT: u32 = 0x0011_5a08;

/// The pointer slots of the result messages (A4-0x6c6c + 4 n, `Msg` order), of the potion
/// descriptions (A4-0x6b4c + 4 n) and of the direction names (A4-0x61be + 4 n), and the slot of
/// `A`'s syllable (A4-0x70b8 + 4 × 0x41: in the image, never catalogued); where the synthetic
/// `game` keeps the texts they point to, 0x10 bytes apart (no catalogued site there).
const MESSAGE_SLOTS: u32 = crate::image::A4 - 0x6c6c;
const POTION_SLOTS: u32 = crate::image::A4 - 0x6b4c;
const DIRECTION_SLOTS: u32 = crate::image::A4 - 0x61be;
const SYLLABLE_A: u32 = crate::image::A4 - 0x70b8 + 4 * 0x41;
const SPELL_LABELS: u32 = crate::image::IMAGE_BASE + 0x400;

/// The synthetic syllable of `A` (the test's own): 3 bytes or more, so a print hook would find
/// it were it catalogued.
const SYLLABLE: &[u8] = b"Zsyl";

/// The synthetic result messages (`Zr` and two digits, `Msg` order).
fn message_texts() -> Vec<Vec<u8>> {
    (0..13).map(|n| format!("Zr{n:02}").into_bytes()).collect()
}

/// The synthetic potion descriptions (`Zd` and a digit, by the potion's frame).
fn potion_texts() -> Vec<Vec<u8>> {
    (0..8).map(|n| format!("Zd{n}").into_bytes()).collect()
}

/// The synthetic direction names (`Zn` and a digit, by direction: 0 north, clockwise).
fn direction_texts() -> Vec<Vec<u8>> {
    (0..8).map(|n| format!("Zn{n}").into_bytes()).collect()
}

/// Magic's sites, each once, in address order.
fn spell_sites() -> Vec<u32> {
    let mut v: Vec<u32> = crate::magic::PRINTED
        .iter()
        .flat_map(|l| l.iter().copied())
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The formats of no word of their own at magic's sites in [`spell_texts`] (the test's own): the
/// result formats, the dispatcher's newline, the book's number, the announcement's formats, the
/// direction's and Locate's formats and the search's newline.
const SPELL_NO_WORD: [(u32, &[u8]); 13] = [
    (SPELL_FMT_S, b"%s"),
    (SPELL_NEWLINE, b"\n"),
    (SPELL_FMT_S_NL, b"%s\n"),
    (SPELL_FMT_NL_S_NL, b"\n%s\n"),
    (SPELL_FMT_NL_S_BANG, b"\n%s!\n"),
    (BOOK_COUNT, b"%d"),
    (SPELL_FMT_SPACE_S, b" %s"),
    (ANNOUNCE_TYPED, b"(%s)"),
    (ANNOUNCE_NAME, b"%s\n"),
    (DIRECTION_NAME, b"%s.\n"),
    (LATITUDE, b"%d"),
    (LONGITUDE, b"%d"),
    (SEARCH_NEWLINE, b"\n"),
];
/// The sites 2 or 4 bytes before the next (a letter each in [`spell_texts`]), and the three
/// lines that print a value (a line with a conversion).
const SPELL_SHORT: [u32; 5] = [CLOSING_QUOTE, SOUTH, NORTH, EAST, WEST];
const SPELL_VALUE: [u32; 3] = [CHARGE_LINE, POTION_LINE, FOOD_LINE];

/// The synthetic text at each of [`spell_sites`] (the test's own; each fits before the next
/// site with its NUL, as the test checks: some lie 2 or 4 bytes apart): [`SPELL_NO_WORD`]'s, a
/// letter at [`SPELL_SHORT`], `Zg`, two digits and a conversion at [`SPELL_VALUE`], `Zg` and two
/// digits at every other.
fn spell_texts() -> SiteLines {
    let lines: SiteLines = spell_sites()
        .into_iter()
        .enumerate()
        .map(|(i, at)| {
            let text = if let Some(&(_, t)) = SPELL_NO_WORD.iter().find(|&&(a, _)| a == at) {
                t.to_vec()
            } else if SPELL_SHORT.contains(&at) {
                b"Z".to_vec()
            } else if at == FOOD_LINE {
                format!("Zg{i:02} %d\n").into_bytes()
            } else if SPELL_VALUE.contains(&at) {
                format!("Zg{i:02} %s\n").into_bytes()
            } else {
                format!("Zg{i:02}").into_bytes()
            };
            (at, text)
        })
        .collect();
    for pair in lines.windows(2) {
        let ((at, text), (next, _)) = (&pair[0], &pair[1]);
        assert!(at + (text.len() as u32) < *next, "the text at {at:#x} fits");
    }
    lines
}

/// The text [`spell_texts`] puts at `at`.
fn spell_text(at: u32) -> String {
    let (_, text) = spell_texts()
        .into_iter()
        .find(|&(a, _)| a == at)
        .expect("a magic site");
    String::from_utf8(text).unwrap()
}

/// [`spell_texts`], and the result messages', the potions', the directions' and `A`'s syllable
/// slots (absolute pointers: no RELOC32) with their texts.
fn spell_lines() -> SiteLines {
    let mut lines = spell_texts();
    let slots = (0..13)
        .map(|n| MESSAGE_SLOTS + 4 * n)
        .chain((0..8).map(|n| POTION_SLOTS + 4 * n))
        .chain((0..8).map(|n| DIRECTION_SLOTS + 4 * n))
        .chain([SYLLABLE_A]);
    let texts = message_texts()
        .into_iter()
        .chain(potion_texts())
        .chain(direction_texts())
        .chain([SYLLABLE.to_vec()]);
    for (i, (slot, text)) in (0u32..).zip(slots.zip(texts)) {
        let at = SPELL_LABELS + 0x10 * i;
        lines.push((slot, at.to_be_bytes().to_vec()));
        lines.push((at, text));
    }
    lines
}

/// A synthetic `game` (one CODE hunk from the image base) holding [`spell_lines`], each string
/// NUL-ended at its site.
fn spell_game() -> Vec<u8> {
    game_of(&spell_lines())
}

/// Move's sites the push case prints, the test's own copies of `objects::commands::push`'s
/// addresses (pinned to its list in the test): the direction prompt, the direction's line and
/// the `\n%s\n` format of the not-possible message.
const PUSH_TO: u32 = 0x0012_85fe;
const PUSH_DIR_LINE: u32 = 0x0012_860c;
const PUSH_NL_LINE_NL: u32 = 0x0012_859a;

/// [`spell_lines`] and Move's lines (the test's own text): the prompt (`Zp1`: the next site lies
/// 4 bytes on), the direction's line and the message format (no word of their own).
fn spellwork_lines() -> SiteLines {
    let mut lines = spell_lines();
    lines.extend([
        (PUSH_TO, b"Zp1".to_vec()),
        (PUSH_DIR_LINE, b"%s.\n".to_vec()),
        (PUSH_NL_LINE_NL, b"\n%s\n".to_vec()),
    ]);
    lines
}

#[test]
fn magics_lines_and_results_are_catalogued_but_its_formats_of_no_word_and_its_syllables_not() {
    let a4 = crate::image::A4;
    // Each file's list in full, as the test's own literals (a constant dropped, added twice or
    // moved fails here): the book page's two formats (its line, a panel's, is not listed); Cast's
    // and the books'; the creature spells'; the
    // detections'; the dispatcher's; the effects' (with a torch's burned-out line); the typed
    // spell's; the object spells'; the spells of their own; the travel spells'.
    assert_eq!(
        crate::magic::PRINTED.map(<[u32]>::to_vec),
        [
            vec![0x0011_58b0, 0x0011_58ac],
            vec![
                0x0011_5b36,
                0x0011_5a9a,
                0x0011_5ab2,
                0x0011_5ad0,
                0x0011_5ada,
                0x0011_5ae0,
                0x0011_58b0,
                0x0011_5a62,
                0x0011_5ae6,
                0x0011_5ae8,
                0x0011_5b0a,
                0x0011_5b26,
            ],
            vec![0x0011_59dc, 0x0011_5a66],
            vec![
                0x0011_5978,
                0x0011_5994,
                0x0011_59a8,
                0x0011_59ba,
                0x0011_595e,
                0x0011_596e,
            ],
            vec![0x0011_58ba],
            vec![
                0x0011_58b0,
                0x0011_58ba,
                0x0011_58ca,
                0x0011_58ce,
                0x0011_58e4,
                0x0011_58d4,
                0x0011_58bc,
                a4 - 0x756f,
            ],
            vec![a4 - 0x7387, 0x0010_4ffc],
            vec![
                0x0011_59f8,
                0x0011_5a08,
                0x0011_5a14,
                0x0011_5a28,
                0x0011_5a38,
                0x0011_5a48,
                0x0011_5a54,
                0x0011_5a5c,
                0x0011_5a62,
            ],
            vec![
                0x0011_59ca,
                0x0011_58ea,
                0x0011_5a08,
                0x0011_5a8a,
                0x0011_5a94,
            ],
            vec![
                0x0011_5a1e,
                0x0011_5a72,
                0x0011_5a78,
                0x0011_5a7a,
                0x0011_5a7c,
                0x0011_5a82,
                0x0011_5a86,
            ],
        ],
        "magic's lists"
    );
    let labels: Vec<u32> = (0..13)
        .map(|n| MESSAGE_SLOTS + 4 * n)
        .chain((0..8).map(|n| POTION_SLOTS + 4 * n))
        .chain((0..8).map(|n| DIRECTION_SLOTS + 4 * n))
        .collect();
    assert_eq!(crate::magic::LABELS, labels.as_slice(), "magic's labels");
    let cat = Catalogue::from_game(&spell_game());
    let lines = spell_texts();
    assert_eq!(lines.len(), 49, "magic's sites, each once");
    for (at, text) in &lines {
        let want = if SPELL_NO_WORD.iter().any(|&(a, _)| a == *at) || SPELL_SHORT.contains(at) {
            None
        } else if SPELL_VALUE.contains(at) {
            Some(Kind::Format)
        } else {
            Some(Kind::Message)
        };
        assert_eq!(cat.of(text).map(|e| e.kind), want, "the string at {at:#x}");
    }
    for text in message_texts()
        .iter()
        .chain(&potion_texts())
        .chain(&direction_texts())
    {
        assert_eq!(cat.of(text).map(|e| e.kind), Some(Kind::Label), "a label");
    }
    assert!(
        cat.of(SYLLABLE).is_none() && cat.label_of(SYLLABLE).is_none(),
        "the syllable is not catalogued"
    );
    assert_eq!(
        cat.len(),
        28 + 3 + 29,
        "28 lines, the three with a value, 13 messages, 8 potions and 8 directions"
    );
}

/// A potion of description 3 (type 0x113, frame 3) and its tile (4 × type + frame).
const POTION_TF: u16 = 0x113 | 3 << 10;
const POTION_TILE: u16 = 4 * 0x113 + 3;

/// Party member `Zm` (actor 1, a party member: status 0x80) at (100, 100) leading a party of one,
/// slots linked, the test text layer; magic's tables hold [`message_texts`], [`potion_texts`] and
/// `A`'s syllable, the potion's tile is a magic tile; tiles 4 × type + frame, no look table
/// (every object is named `zo`, the nothing word).
fn caster_world() -> State {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    s.objects.type_frame[1] = 0x17a;
    s.objects.location[1] = crate::objects::pack_xyz(100, 100, 0);
    s.actors.attrs.status[1] = 0x80;
    let p = &mut s.actors.party;
    p.size = 1;
    p.members[0] = 1;
    p.names = vec!["Zm".into()];
    s.map.centre = s.objects.location[1];
    crate::objects::init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    s.tables.basetile.base = (0..1024).map(|t| t * 4).collect();
    s.objects.defs.nothing_lower = "zo".into();
    let t = &mut s.magic.tables;
    t.messages = message_texts();
    t.potions = potion_texts();
    t.syllables = vec![SYLLABLE.to_vec()];
    t.magic_tiles = vec![POTION_TILE];
    s
}

/// What [`spellwork`] runs: the Cast command, Detect Magic (spell 1) with its target chosen,
/// the key `A` under the spell cursor, the search of the typed syllables, or Wind Change (spell
/// 0x39), Telekinesis (spell 0x15, its target chosen) or Move (the object commands' push)
/// answered with the up key (north, direction 0).
#[derive(Clone, Copy, Debug)]
enum Spellwork {
    Cast,
    Detect,
    Syllable,
    Search,
    Wind,
    Telekinesis,
    Push,
}

/// `work` run on [`caster_world`] after `setup`, in an engine over [`spell_game`], with `pack`
/// installed over its catalogue (or no pack): the scroll's text and the counts.
fn spellwork(work: Spellwork, setup: fn(&mut State), pack: Option<Pack>) -> (String, [u32; 2]) {
    let lines = spellwork_lines();
    let game = game_of(&lines);
    let files = Files::new(BTreeMap::from([("game".to_string(), game.clone())]));
    let mut s = caster_world();
    // The object commands read the same image (Move).
    let parts: Vec<(u32, &[u8])> = lines.iter().map(|(at, t)| (*at, t.as_slice())).collect();
    s.objcmds = crate::objects::commands::ObjCommands::from_data(
        crate::objects::commands::GameData::with_bytes(&parts),
    );
    setup(&mut s);
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
        match work {
            Spellwork::Cast => crate::magic::cast(&st, &io).await,
            Spellwork::Detect => crate::magic::dispatch(&st, &io, 1, true).await,
            Spellwork::Syllable => {
                crate::magic::spell_key(&st, &io, 0x41).await;
            }
            Spellwork::Search => {
                crate::magic::find_typed_spell(&st, &io).await;
            }
            Spellwork::Wind => crate::magic::dispatch(&st, &io, 0x39, false).await,
            Spellwork::Telekinesis => crate::magic::dispatch(&st, &io, 0x15, true).await,
            Spellwork::Push => crate::objects::commands::push(&st, &io).await,
        }
    });
    e.tick(&[], 16);
    if let Spellwork::Wind | Spellwork::Telekinesis | Spellwork::Push = work {
        e.tick(
            &[crate::engine::InputEvent::Key(crate::engine::Key::Up)],
            16,
        );
    }
    // Enough display frames for the flashes a spell holds on screen.
    for _ in 0..6000 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished(), "{work:?} ends");
    let s = e.state();
    (scroll(&s).trim_end().to_string(), s.locale.texts.get())
}

/// A pack of a form for magic's lines the cases print, Move's prompt, four result messages, a
/// potion's description and north's name (the test's own), and one for `A`'s syllable, which
/// is never looked up.
fn spell_ko() -> Pack {
    Pack::new(Language::Korean, "test/1", fingerprint([]), [])
        .unwrap()
        .with_strings([
            string(&spell_text(NO_BOOK), "가"),
            string(&spell_text(NO_MAGIC), "나"),
            string(&spell_text(NO_SPELLS), "다"),
            string(&spell_text(NOT_IN_BOOK), "라"),
            string(&spell_text(POTION_LINE), "마 %s\n"),
            string("Zr08", "바"),
            string("Zr05", "사"),
            string("Zr02", "아"),
            string("Zd3", "자"),
            string(&spell_text(DIRECTION_PROMPT), "카"),
            string("Zn0", "타"),
            string("Zp1", "파"),
            string("Zr06", "하"),
            string("Zsyl", "차"),
        ])
        .unwrap()
}

/// One case: its name, what runs, the world's setup, then the scroll with the pack and the
/// strings it found there, and the scroll without a pack.
type SpellCase<'a> = (&'a str, Spellwork, fn(&mut State), String, u32, String);

#[test]
fn magics_refusals_results_potions_and_directions_print_the_packs_forms_and_else_the_english() {
    for at in [PUSH_TO, PUSH_DIR_LINE, PUSH_NL_LINE_NL] {
        assert!(
            crate::objects::commands::PRINTED
                .iter()
                .any(|l| l.contains(&at)),
            "{at:#x} is in Move's list"
        );
    }
    let nothing: fn(&mut State) = |_| {};
    let empty_book: fn(&mut State) = |s| {
        crate::objects::create_in(s, crate::magic::SPELLBOOK, crate::objects::READIED, 0, 1);
    };
    let negated: fn(&mut State) = |s| {
        crate::objects::create_in(s, crate::magic::SPELLBOOK, crate::objects::READIED, 0, 1);
        s.actors.timers[13] = 1;
    };
    let no_target: fn(&mut State) = |s| {
        s.ui.input.selection = -1;
    };
    let potion: fn(&mut State) = |s| {
        let at = crate::objects::create_on_map(&mut s.objects, POTION_TF, 0, 0, 102, 100, 0);
        assert!(at >= 0x100, "an object slot for the potion");
        s.ui.input.selection = at;
        s.ui.input.target = (102, 100);
    };
    let typed: fn(&mut State) = |s| {
        s.magic.typed_mask = 1;
    };
    // The up arrow's raw key gives north's direction code (the test text layer has no arrows).
    let up_arrow: fn(&mut State) = |s| {
        s.ui.tables.keymap[0x4c] = [0x148; 4];
    };
    // An object of weight 5 two tiles east of the leader, selected and targeted for Telekinesis.
    let weighty: fn(&mut State) = |s| {
        s.ui.tables.keymap[0x4c] = [0x148; 4];
        s.tables.tileflags.weight = vec![0; 0x400];
        s.tables.tileflags.weight[0x77] = 5;
        let at = crate::objects::create_on_map(&mut s.objects, 0x77, 0, 1, 102, 100, 0);
        assert!(at >= 0x100, "an object slot");
        s.ui.input.selection = at;
        s.ui.input.target = (102, 100);
    };
    // A weightless object one tile east of the leader, selected and targeted for Move.
    let beside: fn(&mut State) = |s| {
        s.ui.tables.keymap[0x4c] = [0x148; 4];
        let at = crate::objects::create_on_map(&mut s.objects, 0x30, 0, 0, 101, 100, 0);
        assert!(at >= 0x100, "an object slot");
        s.ui.input.selection = at;
        s.ui.input.target = (101, 100);
    };
    let cases: [SpellCase; 9] = [
        // Cast refused: no book readied, the books' magic negated, no spell in the book.
        (
            "no book",
            Spellwork::Cast,
            nothing,
            "가".into(),
            1,
            spell_text(NO_BOOK),
        ),
        (
            "no magic",
            Spellwork::Cast,
            negated,
            "나".into(),
            1,
            spell_text(NO_MAGIC),
        ),
        (
            "no spells",
            Spellwork::Cast,
            empty_book,
            "다".into(),
            1,
            spell_text(NO_SPELLS),
        ),
        // Detect Magic with no target: the dispatcher's newline, then three result messages
        // through three formats (`%s`, `%s\n`, `\n%s\n`; no word: as they are), each a label
        // read for display.
        (
            "no target",
            Spellwork::Detect,
            no_target,
            "\n바사\n\n아".into(),
            3,
            "\nZr08Zr05\n\nZr02".into(),
        ),
        // On a potion: the prompt, the object's name, then the potion line with the
        // description through `%s` (a label read for display).
        (
            "a potion",
            Spellwork::Detect,
            potion,
            "\n바zo\n마 자".into(),
            3,
            format!("\nZr08zo\n{}", spell_text(POTION_LINE).replace("%s", "Zd3"))
                .trim_end()
                .into(),
        ),
        // A typed spell not in the book: its line, then the search's newline (no word).
        (
            "not in the book",
            Spellwork::Search,
            typed,
            "라".into(),
            1,
            spell_text(NOT_IN_BOOK),
        ),
        // Wind Change: the dispatcher's newline, the direction prompt, then north's name (a label
        // read for display) through the name format (`%s.\n`, no word: as it is).
        (
            "a direction",
            Spellwork::Wind,
            up_arrow,
            "\n카타.".into(),
            2,
            format!("\n{}Zn0.", spell_text(DIRECTION_PROMPT)),
        ),
        // Move: the object's name, the prompt, then north's name (a label read for display)
        // through the direction's line (`%s.\n`, no word: as it is), then the not-possible
        // message (a weightless object) through `\n%s\n`.
        // Telekinesis: the dispatcher's newline, the prompt for a target and the object's name,
        // the direction prompt, then north's name (a label read for display) through `%s\n`.
        (
            "a telekinesis",
            Spellwork::Telekinesis,
            weighty,
            "\n바zo\n카타".into(),
            3,
            format!("\nZr08zo\n{}Zn0", spell_text(DIRECTION_PROMPT)),
        ),
        (
            "a push",
            Spellwork::Push,
            beside,
            "zo\n파타.\n\n하".into(),
            3,
            "zo\nZp1Zn0.\n\nZr06".into(),
        ),
    ];
    for (case, work, setup, ko, found, en) in cases {
        assert_eq!(
            spellwork(work, setup, Some(spell_ko())),
            (ko, [found, 0]),
            "{case}: the pack's forms"
        );
        assert_eq!(
            spellwork(work, setup, None),
            (en, [0, 0]),
            "{case}: the English"
        );
    }
}

#[test]
fn a_typed_syllable_stays_english_with_a_packs_form_for_it() {
    // `A` under the spell cursor prints its syllable as a format: in the image, but no site
    // lists it, so the print hook does not look it up (nothing counted) even with a form.
    let (with, counts) = spellwork(Spellwork::Syllable, |_| {}, Some(spell_ko()));
    let (without, none) = spellwork(Spellwork::Syllable, |_| {}, None);
    assert!(with.starts_with("Zsyl"), "the syllable as it is");
    assert_eq!((with, counts), (without, none), "as without a pack");
    assert_eq!(counts, [0, 0]);
}

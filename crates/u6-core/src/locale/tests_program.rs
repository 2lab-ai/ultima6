//! Slice K4 (spec 08): the pack's strings of the `u` program on synthetic strings (the test's own
//! text): a round trip and a K3 pack's file unchanged, each refusal of the reader, the `printf`
//! codes of `u` and the units, the check at install (codes or a length that differ, or the keys
//! of a prompt not kept, refuse the pack, naming the `u` part, the locale left as it was; a
//! string the catalogue lacks is kept, never looked up, counted nowhere, and the pack installs),
//! the keys standing alone, the lookup (a hit, a miss counted, a string under 3 bytes or
//! uncatalogued neither), and the export's `u` with its keys, read back.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::program::{self, Catalogue, Role};
use super::*;
use crate::converse::disasm::catalog::hash64;

const SENTINEL: &str = "Zq4ProgramSentinel";

/// The test's catalogue: a passage with a colour and a flush, a prompt with the name, a transfer
/// line.
fn catalogue() -> Catalogue {
    Catalogue::from_entries([
        (Role::Story, b"%F%BThe storm rages.\n%P".to_vec()),
        (
            Role::Creation,
            b"By what name art thou called, %s?".to_vec(),
        ),
        (Role::Transfer, b"Strength\nDexterity\n".to_vec()),
    ])
}

fn string(english: &[u8], text: &str) -> (u64, StringText) {
    (
        hash64(english),
        StringText {
            len: english.len() as u32,
            text: text.as_bytes().to_vec(),
        },
    )
}

/// A pack (no runs) of `u` strings.
fn pack(strings: impl IntoIterator<Item = (u64, StringText)>) -> Pack {
    Pack::new(Language::Korean, "test/1", fingerprint([]), [])
        .unwrap()
        .with_program(strings)
        .unwrap()
}

fn korean() -> Pack {
    pack([
        string(b"%F%BThe storm rages.\n%P", "%F%B폭풍이 몰아치는도다.\n%P"),
        string(
            b"By what name art thou called, %s?",
            "그대 이름은 무엇이오, %s?",
        ),
    ])
}

fn file(json: &str) -> Vec<u8> {
    [&b"U6LP\n"[..], json.as_bytes()].concat()
}

fn json_with(extra: &str) -> String {
    format!(
        "{{\"format\":\"u6.locale-pack/1\",\"language\":\"ko\",\"tool\":\"test/1\",\"base\":{{\"converse\":\"{:016x}\"}},\"converse\":[]{extra}}}",
        fingerprint([])
    )
}

fn u_json(hash: &str, len: u32, text: &str) -> String {
    format!("{{\"hash\":\"{hash}\",\"len\":{len},\"text\":\"{text}\"}}")
}

#[test]
fn a_pack_with_u_strings_round_trips_and_a_k3_pack_is_its_file() {
    let p = korean();
    let bytes = p.to_bytes();
    assert_eq!(Pack::parse(&bytes), Ok(p.clone()));
    assert_eq!((p.program_strings(), p.strings(), p.runs()), (2, 0, 0));
    assert!(p.program(hash64(b"%F%BThe storm rages.\n%P")).is_some());
    assert!(
        p.string(hash64(b"%F%BThe storm rages.\n%P")).is_none(),
        "not a game string"
    );
    let text = String::from_utf8(bytes[5..].to_vec()).unwrap();
    assert!(
        text.contains(",\"u\":[{\"hash\":") && text.ends_with("]}"),
        "the u map last"
    );
    // A pack without them writes none: a K3 pack's file, byte for byte.
    let k3 = Pack::new(Language::Korean, "test/1", 7, [])
        .unwrap()
        .with_look([(
            3,
            RunText {
                len: 4,
                hash: hash64(b"lamp"),
                text: "등".as_bytes().to_vec(),
            },
        )])
        .unwrap();
    let k3_bytes = k3.to_bytes();
    assert_eq!(
        k3_bytes,
        file(&format!(
            "{{\"format\":\"u6.locale-pack/1\",\"language\":\"ko\",\"tool\":\"test/1\",\"base\":{{\"converse\":\"0000000000000007\"}},\"converse\":[],\"look\":[{{\"index\":3,\"len\":4,\"hash\":\"{:016x}\",\"text\":\"등\"}}]}}",
            hash64(b"lamp")
        ))
    );
    assert_eq!(Pack::parse(&k3_bytes), Ok(k3));
}

#[test]
fn each_broken_rule_of_the_u_map_refuses_the_pack() {
    let u = |e: &str| json_with(&format!(",\"u\":[{e}]"));
    let h = "0123456789abcdef";
    for (json, want) in [
        (u(&u_json(h, 2, "가")), PackError::ProgramKey),
        (u(&u_json(h, 512, "가")), PackError::ProgramKey),
        (
            u(&u_json(h, 3, &"가".repeat(program::MAX_TEXT_BYTES / 3 + 1))),
            PackError::ProgramKey,
        ),
        (u(&u_json(h, 3, "中")), PackError::Text),
        (u(&u_json(h, 3, "가\\r")), PackError::Text),
        (
            u(&format!("{},{}", u_json(h, 3, "가"), u_json(h, 4, "나"))),
            PackError::Duplicate,
        ),
        (u(&u_json("0123", 3, "가")), PackError::Hash),
        // Every hash is read before an object name is checked: the hash of a string of u first.
        (
            json_with(&format!(
                ",\"look\":[{{\"index\":1,\"len\":0,\"hash\":\"{h}\",\"text\":\"칼\"}}],\"u\":[{}]",
                u_json("0123", 3, "가")
            )),
            PackError::Hash,
        ),
        (
            u(&u_json(h, 3, &format!("{SENTINEL}\\u0001"))),
            PackError::Text,
        ),
        (
            u(&u_json(h, 3, "가").replace("\"len\"", "\"note\":1,\"len\"")),
            PackError::Shape,
        ),
    ] {
        assert_eq!(Pack::parse(&file(&json)), Err(want), "{want:?}");
        assert!(!want.to_string().contains(SENTINEL));
    }
    let longest = u(&u_json(h, 511, &"가".repeat(program::MAX_TEXT_BYTES / 3)));
    assert_eq!(
        Pack::parse(&file(&longest)).map(|p| p.program_strings()),
        Ok(1)
    );
    assert_eq!(
        PackError::ProgramKey.to_string(),
        "an opening or creation string in it is not one the u program prints"
    );
}

#[test]
fn the_codes_are_the_u_printfs_and_a_unit_has_words_of_its_own() {
    let d = |f: &[u8]| program::directives(f);
    assert_eq!(
        d(b"%F%B%X%Ya %s and %d%%%P"),
        [&b"%F"[..], b"%B", b"%X", b"%Y", b"%s", b"%d", b"%%", b"%P"]
    );
    assert_eq!(d(b"50%"), [&b"%"[..]], "a % that ends the format alone");
    assert_eq!(d(b"%q\0%s"), [&b"%q"[..]], "a NUL ends the format");
    assert!(d(b"plain").is_empty());
    for (text, unit) in [
        (&b"The storm rages."[..], true),
        (b"Hi", false),
        (b"%s\n%s\n%s\n", false),
        (b"%d %d", false),
        (b"IV", false),
        (b"Name:\r", false),
        (b"Caf\xe9", false),
        (b"\x01abc", false),
    ] {
        assert_eq!(program::is_unit(text), unit, "{text:?}");
    }
    assert!(program::is_unit(&[b'a'; 511]) && !program::is_unit(&[b'a'; 512]));
    let c = Catalogue::from_entries([
        (Role::Gypsy, b"A question.".to_vec()),
        (Role::Story, b"A question.".to_vec()),
        (Role::Story, b"%s".to_vec()),
    ]);
    assert_eq!(c.len(), 1, "once, the units only");
    assert_eq!(
        c.of(b"A question.").map(|e| e.role),
        Some(Role::Gypsy),
        "the first site's"
    );
}

#[test]
fn at_install_a_u_string_whose_codes_differ_refuses_the_pack_naming_the_u_part() {
    let install = |p: Pack| {
        let mut s = State::empty();
        let r = install_parts(
            &mut s,
            &Files::default(),
            Rc::new(p),
            None,
            strings::Catalogue::default(),
            catalogue(),
        );
        (r, s.locale.pack.is_some())
    };
    assert_eq!(install(korean()), (Ok(()), true));
    let refused = Mismatch {
        part: Part::Program,
    };
    for ko in [
        "%B%F폭풍이 몰아치는도다.\n%P",
        "%F%B폭풍이 몰아치는도다.\n",
        "%F%B폭풍이 몰아치는도다.\n%P%P",
        "%F%B폭풍이 몰아치는도다 %s.\n%P",
        "%F%%B폭풍이 몰아치는도다.\n%P",
    ] {
        let p = pack([string(b"%F%BThe storm rages.\n%P", ko)]);
        assert_eq!(install(p), (Err(refused), false), "{ko}");
    }
    assert_eq!(refused.part.name(), "u");
    assert_eq!(
        refused.to_string(),
        "its opening and creation part does not keep the u program's format codes"
    );
}

/// [`install_parts`] of `p` on `s` with the test's catalogue.
fn install_on(s: &mut State, p: Pack) -> Result<(), Mismatch> {
    install_parts(
        s,
        &Files::default(),
        Rc::new(p),
        None,
        strings::Catalogue::default(),
        catalogue(),
    )
}

#[test]
fn a_catalogued_u_string_of_another_length_refuses_the_pack_naming_u_and_the_locale_stays_as_it_was()
 {
    let mut s = State::empty();
    assert_eq!(install_on(&mut s, korean()), Ok(()));
    let before = (s.locale.clone(), s.ui.text.clone());
    // The catalogue's hash with its codes kept but another English length.
    let (hash, mut t) = string(
        b"%F%BThe storm rages.
%P",
        "%F%B폭풍.
%P",
    );
    for len in [t.len - 1, t.len + 1] {
        t.len = len;
        let wrong = pack([(hash, t.clone())]);
        let r = install_on(&mut s, wrong);
        assert_eq!(
            r,
            Err(Mismatch {
                part: Part::Program
            }),
            "{len}"
        );
        assert_eq!(r.unwrap_err().part.name(), "u");
        assert_eq!((s.locale.clone(), s.ui.text.clone()), before, "as it was");
    }
}

#[test]
fn a_u_string_the_catalogue_lacks_is_kept_never_looked_up_counted_nowhere_and_the_pack_installs() {
    // A pack outlives a narrower catalogue of a later engine (the game strings' rule): a string
    // whose hash this `u`'s catalogue lacks is not checked (its codes differ from its English
    // here, and its length is another's), stays in the pack, and is never looked up.
    let english = b"Not in this u, %s.";
    let (hash, mut t) = string(english, "이 u에는 없소.");
    t.len += 4;
    let mut s = State::empty();
    assert_eq!(install_on(&mut s, pack([(hash, t.clone())])), Ok(()));
    let installed = s.locale.pack.clone().unwrap();
    assert_eq!(installed.program(hash), Some(&t), "kept");
    let counts = Counts::default();
    assert_eq!(
        program::shown(&installed, &catalogue(), &counts, english),
        None,
        "never looked up"
    );
    assert_eq!(counts.get(), [0, 0], "counted nowhere");
    assert_eq!(s.locale.program.get(), [0, 0]);
}

/// The test's keyed prompts: a question whose answers stand alone, a sex question whose keys are
/// words' first letters, a sheet label keyed by a table, and an unkeyed passage.
fn keyed() -> Catalogue {
    Catalogue::from_keyed([
        (
            Role::Gypsy,
            b"Wouldst thou A) give alms, or B) keep thy coin?".to_vec(),
            b"AB".to_vec(),
        ),
        (
            Role::Creation,
            b"Art thou Male or Female?
%P"
            .to_vec(),
            b"MF".to_vec(),
        ),
        (
            Role::Transfer,
            b"Name
Sex
"
            .to_vec(),
            b"NSCA".to_vec(),
        ),
        (
            Role::Story,
            b"A storm. A) is not a key here.".to_vec(),
            Vec::new(),
        ),
    ])
}

#[test]
fn the_keys_a_prompt_shows_standing_alone_are_letters_next_to_no_letter_outside_the_codes() {
    let shown = |t: &[u8], k: &[u8]| program::keys_shown(t, k);
    assert_eq!(shown(b"Wouldst thou A) give, or B) keep?", b"AB"), b"AB");
    assert_eq!(shown(b"B) first, then A) and A) again", b"AB"), b"BA");
    assert_eq!(shown(b"(M)ale or (F)emale", b"MF"), b"MF");
    assert_eq!(shown(b"N)ew", b"N"), b"N");
    assert!(shown(b"Male or Female", b"MF").is_empty(), "first letters");
    assert!(
        shown(b"%B%F the %A code", b"ABF").is_empty(),
        "codes are not keys"
    );
    assert!(shown(b"a) lower", b"A").is_empty(), "upper case only");
    assert_eq!(shown(b"%sA) after a name", b"A"), b"A");
    assert_eq!(shown("가A)나".as_bytes(), b"A"), b"A", "next to Hangul");
    assert!(
        shown(b"AB) together", b"AB").is_empty(),
        "a word of two keys"
    );
    let kept = |en: &[u8], ko: &str, k: &[u8]| program::keys_kept(en, ko.as_bytes(), k);
    let q = b"Wouldst thou A) give alms, or B) keep thy coin?";
    assert!(kept(q, "그대는 A) 베풀겠는가, B) 지키겠는가?", b"AB"));
    assert!(
        !kept(q, "그대는 A) 베풀겠는가, 지키겠는가?", b"AB"),
        "B dropped"
    );
    assert!(
        !kept(q, "그대는 B) 지키겠는가, A) 베풀겠는가?", b"AB"),
        "reordered"
    );
    assert!(
        kept(q, "그대는가A) 베풀겠는가, B) 지키겠는가?", b"AB"),
        "next to Hangul"
    );
    assert!(
        !kept(q, "그대는 SA) 베풀겠는가, B)?", b"AB"),
        "inside a word"
    );
    assert!(
        kept(b"Male or Female?", "남(M) 여(F)?", b"MF"),
        "added keys are not counted"
    );
    assert!(
        kept(b"Male or Female?", "남자인가 여자인가?", b"MF"),
        "first letters not checked"
    );
    assert!(kept(q, "아무 말", b""), "no keys");
}

#[test]
fn at_install_a_keyed_prompt_that_drops_or_reorders_its_keys_refuses_the_pack_naming_u() {
    let c = keyed();
    let install = |entries: &[(&[u8], &str)]| {
        let mut s = State::empty();
        let p = pack(entries.iter().map(|&(en, ko)| string(en, ko)));
        let r = install_parts(
            &mut s,
            &Files::default(),
            Rc::new(p),
            None,
            strings::Catalogue::default(),
            c.clone(),
        );
        (r, s.locale.pack.is_some())
    };
    let q: &[u8] = b"Wouldst thou A) give alms, or B) keep thy coin?";
    let refused = (
        Err(Mismatch {
            part: Part::Program,
        }),
        false,
    );
    assert_eq!(
        install(&[(q, "그대는 A) 베풀겠는가, B) 지키겠는가?")]),
        (Ok(()), true)
    );
    assert_eq!(
        install(&[(q, "그대는 A) 베풀겠는가, 지키겠는가?")]),
        refused
    );
    assert_eq!(
        install(&[(q, "그대는 B) 지키겠는가, A) 베풀겠는가?")]),
        refused
    );
    assert_eq!(install(&[(q, "그대는 베풀겠는가, 지키겠는가?")]), refused);
    // Keys the English shows only inside words are not checked; an unkeyed entry neither.
    let sex: &[u8] = b"Art thou Male or Female?
%P";
    assert_eq!(
        install(&[(
            sex,
            "남자인가 여자인가?
%P"
        )]),
        (Ok(()), true)
    );
    assert_eq!(
        install(&[(
            b"Name
Sex
",
            "이름
성별
"
        )]),
        (Ok(()), true)
    );
    let story: &[u8] = b"A storm. A) is not a key here.";
    assert_eq!(install(&[(story, "폭풍이로다.")]), (Ok(()), true));
}

#[test]
fn the_catalogue_keeps_each_sites_keys_once_its_letters_only() {
    let q = b"Wouldst thou A) give alms, or B) keep thy coin?".to_vec();
    let c = Catalogue::from_keyed([
        (Role::Story, q.clone(), Vec::new()),
        (Role::Gypsy, q.clone(), b"AB".to_vec()),
        (Role::Gypsy, q.clone(), b"BA".to_vec()),
        (
            Role::Transfer,
            b"Name
Sex
"
            .to_vec(),
            b"N SCAa".to_vec(),
        ),
    ]);
    assert_eq!(c.len(), 2);
    let e = c.of(&q).unwrap();
    assert_eq!(
        (e.role, e.keys.as_slice()),
        (Role::Story, &b"AB"[..]),
        "the first role, every site's keys"
    );
    assert_eq!(
        c.of(b"Name
Sex
")
            .unwrap()
            .keys,
        b"NSCA",
        "upper-case letters, each once"
    );
}

#[test]
fn a_u_string_is_looked_up_by_content_when_catalogued_a_miss_counted_and_nothing_else() {
    let (p, c, counts) = (korean(), catalogue(), Counts::default());
    let shown = |english: &[u8]| program::shown(&p, &c, &counts, english);
    assert_eq!(
        shown(b"%F%BThe storm rages.\n%P"),
        Some("%F%B폭풍이 몰아치는도다.\n%P".as_bytes().to_vec())
    );
    assert_eq!(counts.get(), [1, 0]);
    assert_eq!(
        shown(b"Strength\nDexterity\n"),
        None,
        "catalogued, not in the pack"
    );
    assert_eq!(counts.get(), [1, 1], "a miss");
    assert_eq!(shown(b"Hi"), None, "under 3 bytes");
    assert_eq!(shown(b"Not in this u, %s."), None, "not catalogued");
    assert_eq!(counts.get(), [1, 1], "neither counted");
    assert_eq!(counts.take(), [1, 1]);
    assert_eq!(counts.get(), [0, 0], "taken");
    counts.add([2, 3]);
    assert_eq!(counts.get(), [2, 3]);
}

#[test]
fn the_export_lists_the_u_strings_with_their_roles_and_keys_and_reads_back() {
    let mut keys = strings::Catalogue::default().export();
    keys.u = keyed().export();
    let json = keys.to_json();
    assert!(json.contains("?\",\"keys\":\"AB\"}") && json.contains("\"keys\":\"NSCA\"}"));
    assert_eq!(json.matches("\"keys\"").count(), 3, "none for the unkeyed");
    assert_eq!(strings::Export::from_json(&json), Ok(keys.clone()));
    for bad in ["ab", "AA", "A1", "A\u{1b}"] {
        let mut k = keys.clone();
        k.u[0].keys = bad.into();
        assert_eq!(
            strings::Export::from_json(&k.to_json()),
            Err("a string of the u program's text, length and hash disagree"),
            "{bad}"
        );
    }
    let mut e = strings::Catalogue::default().export();
    e.u = catalogue().export();
    let json = e.to_json();
    assert!(json.contains("\"u\":[{\"kind\":\"story\",\"hash\":\""));
    assert!(json.contains("\"kind\":\"creation\"") && json.contains("\"kind\":\"transfer\""));
    assert!(!json.contains("\"keys\""), "no keys: none written");
    assert_eq!(strings::Export::from_json(&json), Ok(e.clone()));
    let mut bad = e.clone();
    bad.u[0].len += 1;
    assert_eq!(
        strings::Export::from_json(&bad.to_json()),
        Err("a string of the u program's text, length and hash disagree")
    );
    let mut short = e;
    short.u[0].text = "ab".into();
    short.u[0].len = 2;
    short.u[0].hash = hash64(b"ab");
    assert!(
        strings::Export::from_json(&short.to_json()).is_err(),
        "under 3 bytes"
    );
    // An export without them writes none (K3's form); no `u` file gives none and a `u` that
    // does not read fails with a fixed sentence.
    assert!(!strings::Catalogue::default().to_json().contains("\"u\""));
    assert_eq!(program::export(&Files::default()), Ok(Vec::new()));
    let broken = Files::new(BTreeMap::from([(
        "U6/U".to_string(),
        b"not a hunk".to_vec(),
    )]));
    assert_eq!(program::export(&broken), Err("the u program does not read"));
    assert!(strings::reads("U6/U") && strings::reads("u") && !strings::reads("u.bin"));
}

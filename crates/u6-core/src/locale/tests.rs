//! The pack reader and the load-time check on synthetic packs and archives (the test's own
//! text): a round trip, each refusal with its fixed sentence and nothing of the file in it, the
//! limits, and every way a pack can disagree with the game files.

use super::*;
use crate::converse::disasm::catalog::hash64;

const SENTINEL: &str = "Zq8LocaleSentinel";

/// A synthetic script: a header, a look text and a greeting of the test's own.
fn script() -> Vec<u8> {
    b"\xff\x03Ann\xf1a woman\xf2Hello there.\xb6".to_vec()
}

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

fn run(text: &str, script: &[u8], at: usize, len: usize) -> RunText {
    RunText {
        len: len as u32,
        hash: hash64(&script[at..at + len]),
        text: text.as_bytes().to_vec(),
    }
}

/// The pack of the greeting ("Hello there." at 14) over `script`.
fn pack(script: &[u8]) -> Pack {
    Pack::new(
        Language::Korean,
        "test/1",
        fingerprint([(b'a', 3, hash64(script))]),
        [((b'a', 3, 14), run("안녕하시오.", script, 14, 12))],
    )
    .unwrap()
}

/// The pack file of `json` (magic and line feed first).
fn file(json: &str) -> Vec<u8> {
    [&b"U6LP\n"[..], json.as_bytes()].concat()
}

/// The JSON of a one-run pack with these fields (as text) changed.
fn json_with(changes: &[(&str, &str)]) -> String {
    let mut fields: Vec<(&str, String)> = vec![
        ("format", "\"u6.locale-pack/1\"".into()),
        ("language", "\"ko\"".into()),
        ("tool", "\"test/1\"".into()),
        ("base", "{\"converse\":\"0123456789abcdef\"}".into()),
        (
            "converse",
            "[{\"archive\":\"a\",\"item\":3,\"offset\":14,\"len\":12,\"hash\":\"0123456789abcdef\",\"text\":\"안녕\"}]"
                .into(),
        ),
    ];
    for (k, v) in changes {
        match fields.iter_mut().find(|(f, _)| f == k) {
            Some(f) => f.1 = (*v).to_string(),
            None => fields.push((k, (*v).to_string())),
        }
    }
    let body: Vec<String> = fields.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    format!("{{{}}}", body.join(","))
}

fn one_run(fields: &str) -> String {
    json_with(&[("converse", &format!("[{{{fields}}}]"))])
}

const RUN: &str =
    "\"archive\":\"a\",\"item\":3,\"offset\":14,\"len\":12,\"hash\":\"0123456789abcdef\"";

#[test]
fn a_pack_round_trips_and_counts_its_runs_and_scripts() {
    let s = script();
    let p = pack(&s);
    let bytes = p.to_bytes();
    assert!(bytes.starts_with(b"U6LP\n{"));
    assert_eq!(Pack::parse(&bytes), Ok(p.clone()));
    assert_eq!((p.runs(), p.scripts()), (1, 1));
    assert_eq!(p.run((b'a', 3, 14)).map(|r| r.len), Some(12));
    assert!(p.run((b'a', 3, 15)).is_none());
    assert!(Pack::parse(&file(&json_with(&[]))).is_ok());
    // JSON whitespace after the object is JSON's; nothing else may follow.
    assert!(Pack::parse(&[bytes.clone(), b"\n".to_vec()].concat()).is_ok());
    assert_eq!(
        Pack::parse(&[bytes, b"x".to_vec()].concat()),
        Err(PackError::Shape)
    );
}

#[test]
fn each_broken_rule_refuses_the_pack_with_its_own_fixed_sentence() {
    let cases: Vec<(Vec<u8>, PackError)> = vec![
        (b"U6LQ\n{}".to_vec(), PackError::NotAPack),
        (b"U6LP{}".to_vec(), PackError::NotAPack),
        (b"".to_vec(), PackError::NotAPack),
        (file("not json"), PackError::Shape),
        (file("[]"), PackError::Shape),
        (file(&json_with(&[("extra", "1")])), PackError::Shape),
        (
            file(&json_with(&[(
                "base",
                "{\"converse\":\"0123456789abcdef\",\"look\":\"0\"}",
            )])),
            PackError::Shape,
        ),
        (
            file(&one_run(&format!("{RUN},\"text\":\"a\",\"note\":1"))),
            PackError::Shape,
        ),
        (file(&one_run(RUN)), PackError::Shape),
        (
            file(&json_with(&[]).replace("\"format\":\"u6.locale-pack/1\",", "")),
            PackError::Shape,
        ),
        (
            file(&json_with(&[("format", "\"u6.locale-pack/2\"")])),
            PackError::Format,
        ),
        (
            file(&json_with(&[("language", "\"fr\"")])),
            PackError::Language,
        ),
        (file(&json_with(&[("tool", "\"\"")])), PackError::Tool),
        (
            file(&json_with(&[("tool", &format!("\"{}\"", "t".repeat(65)))])),
            PackError::Tool,
        ),
        (file(&json_with(&[("tool", "\"툴\"")])), PackError::Tool),
        (
            file(&json_with(&[(
                "base",
                "{\"converse\":\"0123456789ABCDEF\"}",
            )])),
            PackError::Hash,
        ),
        (
            file(&json_with(&[("base", "{\"converse\":\"0123\"}")])),
            PackError::Hash,
        ),
        (
            file(&one_run(
                "\"archive\":\"a\",\"item\":3,\"offset\":14,\"len\":12,\"hash\":\"xyz\",\"text\":\"a\"",
            )),
            PackError::Hash,
        ),
        (
            file(&one_run(
                "\"archive\":\"c\",\"item\":3,\"offset\":14,\"len\":12,\"hash\":\"0123456789abcdef\",\"text\":\"a\"",
            )),
            PackError::Key,
        ),
        (
            file(&one_run(
                "\"archive\":\"ab\",\"item\":3,\"offset\":14,\"len\":12,\"hash\":\"0123456789abcdef\",\"text\":\"a\"",
            )),
            PackError::Key,
        ),
        (
            file(&one_run(
                "\"archive\":\"a\",\"item\":3,\"offset\":14,\"len\":0,\"hash\":\"0123456789abcdef\",\"text\":\"a\"",
            )),
            PackError::Key,
        ),
        (
            file(&one_run(
                "\"archive\":\"a\",\"item\":3,\"offset\":10239,\"len\":2,\"hash\":\"0123456789abcdef\",\"text\":\"a\"",
            )),
            PackError::Key,
        ),
        (
            file(&one_run(
                "\"archive\":\"a\",\"item\":3,\"offset\":4294967295,\"len\":4294967295,\"hash\":\"0123456789abcdef\",\"text\":\"a\"",
            )),
            PackError::Key,
        ),
        (
            file(&one_run(
                "\"archive\":\"a\",\"item\":70000,\"offset\":1,\"len\":1,\"hash\":\"0123456789abcdef\",\"text\":\"a\"",
            )),
            PackError::Shape,
        ),
        (
            file(&one_run(&format!("{RUN},\"text\":\"a\\u0001b\""))),
            PackError::Text,
        ),
        (
            file(&one_run(&format!("{RUN},\"text\":\"a\\u0000b\""))),
            PackError::Text,
        ),
        (
            file(&one_run(&format!("{RUN},\"text\":\"\\t\""))),
            PackError::Text,
        ),
        (
            file(&one_run(&format!("{RUN},\"text\":\"café\""))),
            PackError::Text,
        ),
        (
            file(&one_run(&format!("{RUN},\"text\":\"中\""))),
            PackError::Text,
        ),
        (
            file(&one_run(&format!("{RUN},\"text\":\"ㄱ\""))),
            PackError::Text,
        ),
        (
            file(&one_run(&format!(
                "{RUN},\"text\":\"{}\"",
                "가".repeat(MAX_TEXT_BYTES / 3 + 1)
            ))),
            PackError::Text,
        ),
        (
            file(&json_with(&[(
                "converse",
                &format!("[{{{RUN},\"text\":\"a\"}},{{{RUN},\"text\":\"b\"}}]"),
            )])),
            PackError::Duplicate,
        ),
    ];
    for (i, (bytes, want)) in cases.iter().enumerate() {
        assert_eq!(Pack::parse(bytes), Err(*want), "case {i}");
    }
    // The sentences name the rule only.
    for e in cases.iter().map(|(_, e)| e) {
        let text = e.to_string();
        assert!(!text.is_empty() && !text.contains(SENTINEL));
    }
    // A sentinel in the file never reaches the refusal.
    let marked = file(&json_with(&[("tool", &format!("\"{SENTINEL}\u{1}\""))]));
    let err = Pack::parse(&marked).unwrap_err();
    assert!(!err.to_string().contains(SENTINEL));
    let marked = file(&format!("{{\"{SENTINEL}\": \"{SENTINEL}\""));
    assert!(
        !Pack::parse(&marked)
            .unwrap_err()
            .to_string()
            .contains(SENTINEL)
    );
}

#[test]
fn the_limits_are_read_before_the_runs() {
    let big = vec![b' '; MAX_PACK_BYTES + 1];
    assert_eq!(Pack::parse(&big), Err(PackError::TooLarge));
    // One run more than a pack may hold: refused as too many, not read run by run.
    let runs: Vec<String> = (0..=MAX_RUNS as u32)
        .map(|i| {
            format!(
                "{{\"archive\":\"{}\",\"item\":{},\"offset\":0,\"len\":1,\"hash\":\"0123456789abcdef\",\"text\":\"a\"}}",
                if i % 2 == 0 { 'a' } else { 'b' },
                i / 2
            )
        })
        .collect();
    let json = json_with(&[("converse", &format!("[{}]", runs.join(",")))]);
    assert_eq!(Pack::parse(&file(&json)), Err(PackError::TooManyRuns));
    // At the limit it reads.
    let json = json_with(&[("converse", &format!("[{}]", runs[..MAX_RUNS].join(",")))]);
    assert_eq!(Pack::parse(&file(&json)).map(|p| p.runs()), Ok(MAX_RUNS));
}

#[test]
fn the_check_takes_only_the_files_the_pack_was_made_from() {
    let s = script();
    let p = pack(&s);
    let lib = archive(&s);
    let ok = |a: &Vec<u8>| p.check(|l| (l == b'a').then_some(a.as_slice()));
    assert_eq!(ok(&lib), Ok(()));
    let bad = Err(Mismatch {
        part: Part::Converse,
    });
    // Another byte anywhere in the script: the fingerprint differs.
    let mut other = s.clone();
    other[8] = b'W';
    assert_eq!(ok(&archive(&other)), bad);
    // No archive, an archive that does not read, no such item.
    assert_eq!(p.check(|_| None), bad);
    assert_eq!(ok(&vec![1, 2, 3]), bad);
    let mut short = lib.clone();
    short[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(ok(&short), bad);
    // The fingerprint right but a run over a byte that is not text (the `f2` before it), past
    // the script's end, or with another hash.
    let base = fingerprint([(b'a', 3, hash64(&s))]);
    let with = |key: RunKey, r: RunText| {
        Pack::new(Language::Korean, "test/1", base, [(key, r)])
            .unwrap()
            .check(|_| Some(lib.as_slice()))
    };
    assert_eq!(with((b'a', 3, 13), run("가", &s, 13, 3)), bad);
    assert_eq!(
        with(
            (b'a', 3, 20),
            RunText {
                len: 40,
                hash: 0,
                text: b"a".to_vec()
            }
        ),
        bad
    );
    assert_eq!(
        with(
            (b'a', 3, 14),
            RunText {
                hash: 1,
                ..run("가", &s, 14, 12)
            }
        ),
        bad
    );
    assert_eq!(with((b'a', 3, 14), run("가", &s, 14, 5)), Ok(()));
    // The mismatch names the part only.
    let text = Mismatch {
        part: Part::Converse,
    }
    .to_string();
    assert!(text.contains("conversation") && !text.contains("Hello"));
}

#[test]
fn the_fingerprint_is_of_the_set_of_scripts() {
    let a = fingerprint([(b'a', 3, 7), (b'b', 1, 9)]);
    assert_eq!(a, fingerprint([(b'b', 1, 9), (b'a', 3, 7), (b'a', 3, 7)]));
    assert_ne!(a, fingerprint([(b'a', 3, 7), (b'b', 1, 8)]));
    assert_ne!(a, fingerprint([(b'a', 3, 7)]));
    // Its bytes: letter, item (LE), hash (LE) per script; the vector the translation tool's
    // test checks too (tools/i18n/translate.test.mjs).
    let mut bytes = vec![b'a', 3, 0];
    bytes.extend_from_slice(&7u64.to_le_bytes());
    assert_eq!(fingerprint([(b'a', 3, 7)]), hash64(&bytes));
    assert_eq!(
        format!("{:016x}", fingerprint([(b'a', 3, 7)])),
        "200c9bb3451c6be0"
    );
}

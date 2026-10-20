//! Typing Korean in a conversation (spec 08 Design 4, slice K5a) on hand-built scripts: Hangul
//! syllables typed as text reach the player's words with a locale pack installed, and the pack's
//! keyword aliases answer as their English keywords, with or without a particle after them,
//! while the English keywords still answer, the blocks are tried in their order and `$Z` shows
//! what was typed. All text here is the test's own (made-up words).

use std::collections::BTreeMap;
use std::rc::Rc;

use super::begin;
use super::disasm::catalog::ScriptUnits;
use super::disasm::trace;
use super::run::{PARTICLES, match_alias};
use super::tests::{run, state, text};
use super::tests_locale::{flow, pack_for, screen, script, talk_files};
use super::text::{ConverseText, MESSAGES, Msg};
use crate::engine::{InputEvent, Key};
use crate::locale::{self, Pack, PackError, Part};
use crate::state::State;
use crate::ui::Language;

/// Two keyword blocks, `job` then `name`, after the player's words; no match prints `No.`.
const BODY: &[u8] = b"\xf7\xefjob\xf6Work.\xb6\xefname\xf6Me $Z.\xb6\xeeNo.\xb6";

/// `text` as the page would send it: a Hangul syllable as text, a line feed as Enter, anything
/// else as its key.
fn typed(text: &str) -> Vec<InputEvent> {
    text.chars()
        .map(|c| match c {
            '\n' => InputEvent::Key(Key::Enter),
            '\u{ac00}'..='\u{d7a3}' => InputEvent::Text(c),
            c => InputEvent::Key(Key::Char(c)),
        })
        .collect()
}

/// The pack of `BODY`'s runs (each as itself, in `language`'s grammar) with the keyword aliases
/// `aliases` (keyword, words) of NPC 3's script.
fn aliased(language: Language, aliases: &[(&str, &[&str])]) -> Pack {
    let script = script(BODY);
    let units = ScriptUnits::build(b'a', 3, &script);
    let lines: BTreeMap<&str, &str> = units
        .runs
        .iter()
        .map(|r| (r.unit.text.as_str(), r.unit.text.as_str()))
        .collect();
    pack_for(&script, language, &lines)
        .with_aliases(aliases.iter().map(|(k, words)| {
            (
                (b'a', 3),
                k.as_bytes().to_vec(),
                words.iter().map(|w| w.as_bytes().to_vec()).collect(),
            )
        }))
        .unwrap()
}

/// A conversation with NPC 3 over `body`, `pack` installed (or none), given `events` one per
/// tick: its control flow and the state after it.
fn talk(body: &[u8], pack: Option<&Pack>, events: Vec<InputEvent>) -> (Vec<String>, State) {
    let script = script(body);
    let f = talk_files(&script);
    let mut s = state(&[]);
    if let Some(p) = pack {
        locale::install(&mut s, &f, Rc::new(p.clone()), None).expect("the pack checks");
    }
    trace::start(false);
    let (_, s) = run!(s, f, events, |cx| begin(cx, 0, 3).await);
    (flow(&trace::stop()), s)
}

/// What the conversation over `BODY` answers to `line` typed with the Korean pack of the
/// aliases 일 (`job`) and 이름 (`name`): the end of the scroll, and the player's words.
fn answer(line: &str) -> (String, Vec<u8>) {
    let pack = aliased(
        Language::Korean,
        &[("job", &["일"]), ("name", &["이름", "성명"])],
    );
    let (_, s) = talk(BODY, Some(&pack), typed(&format!("{line}\n")));
    (screen(&s).trim_end().to_string(), s.converse.input.clone())
}

#[test]
fn a_korean_alias_answers_as_its_keyword_alone_or_with_one_particle_and_dollar_z_shows_what_was_typed()
 {
    for line in ["이름", "이름은", "이름에게", "이름이여", "성명을", "성명"] {
        let (shown, input) = answer(line);
        assert_eq!(input, line.as_bytes(), "the input as typed (UTF-8): {line}");
        assert!(
            shown.contains(&format!("say:{line}")),
            "echoed in the message window: {shown}"
        );
        assert!(
            shown.ends_with(&format!("Me {line}.")),
            "the answer of `name`, `$Z` what was typed: {shown}"
        );
    }
    // Not a particle, two of them, a part of the alias: no keyword.
    for line in ["이름들", "이름은는", "이", "름은"] {
        let (shown, _) = answer(line);
        assert!(shown.ends_with("No."), "{line}: {shown}");
    }
    // Every particle of the list after an alias, longest first in the list.
    let words = [b"\xec\x9d\xb4\xeb\xa6\x84".to_vec()];
    for p in PARTICLES {
        assert!(match_alias(&words, format!("이름{p}").as_bytes(), b" "));
    }
    assert!(PARTICLES.windows(2).all(|w| w[0].len() >= w[1].len()));
    assert!(
        match_alias(&[b"na?e".to_vec()], b"x NA?E", b" "),
        "ASCII folded"
    );
    assert!(
        !match_alias(&[b"na?e".to_vec()], b"name", b" "),
        "`?` is no wildcard"
    );
}

#[test]
fn the_english_keyword_still_answers_and_the_first_block_that_matches_answers() {
    for (line, end) in [
        ("name", "Me name."),
        ("NAME", "Me NAME."),
        // `job`'s block comes first: its alias answers before `name`'s English or alias.
        ("이름 일", "Work."),
        ("name 일은", "Work."),
        ("이름 job", "Work."),
        ("이름 x", "Me 이름 x."),
    ] {
        let (shown, _) = answer(line);
        assert!(shown.ends_with(end), "{line}: {shown}");
    }
}

#[test]
fn hangul_typed_without_a_pack_never_reaches_the_words_and_ascii_typed_as_text_is_its_key() {
    // No pack: the syllables are dropped where they are read; the empty line is the goodbye.
    let (_, en) = talk(BODY, None, typed("이름\n"));
    assert!(en.converse.input.is_ascii(), "no Hangul byte in the words");
    assert!(screen(&en).trim_end().ends_with("No."));
    assert_eq!(
        talk(BODY, None, typed("이름\n")).0,
        talk(BODY, None, typed("\n")).0,
        "the reads of an empty line"
    );
    // Printable ASCII typed as text is the press of its key, with a pack or without.
    let as_text = |line: &str| -> Vec<InputEvent> {
        line.chars()
            .map(|c| match c {
                '\n' => InputEvent::Key(Key::Enter),
                c => InputEvent::Text(c),
            })
            .collect()
    };
    let pack = aliased(Language::Korean, &[("name", &["이름"])]);
    for pack in [None, Some(&pack)] {
        let (keys_flow, keys) = talk(BODY, pack, typed("x name\n"));
        let (text_flow, text) = talk(BODY, pack, as_text("x name\n"));
        assert_eq!(text_flow, keys_flow);
        assert_eq!(screen(&text), screen(&keys));
        assert_eq!(text.converse.input, b"x name");
    }
}

#[test]
fn an_alias_equal_to_its_keyword_changes_nothing() {
    // The identity: every run as itself in English grammar, each keyword its own alias. Every
    // read, the scroll and the words are English's.
    let pack = aliased(Language::English, &[("job", &["job"]), ("name", &["name"])]);
    for line in [
        "name\n",
        "NAME\n",
        "names\n",
        "nam\n",
        "x job name\n",
        "\n",
        "jobname\n",
    ] {
        let (en_flow, en) = talk(BODY, None, typed(line));
        let (id_flow, id) = talk(BODY, Some(&pack), typed(line));
        assert_eq!(id_flow, en_flow, "{line:?}");
        assert_eq!(screen(&id), screen(&en), "{line:?}");
        assert_eq!(id.converse.input, en.converse.input);
    }
}

/// The JSON of `pack`'s file with `aliases` put in as its aliases part.
fn with_part(pack: &Pack, aliases: serde_json::Value) -> Vec<u8> {
    let bytes = pack.to_bytes();
    let mut json: serde_json::Value = serde_json::from_slice(&bytes[5..]).unwrap();
    json["aliases"] = aliases;
    [&bytes[..5], &serde_json::to_vec(&json).unwrap()[..]].concat()
}

/// The file of `pack` (which has no aliases part) with `raw`, written as it is, as its aliases
/// part: JSON a `Value` cannot hold, such as an object with one field twice.
fn with_raw_part(pack: &Pack, raw: &str) -> Vec<u8> {
    let bytes = pack.to_bytes();
    let body = bytes
        .strip_suffix(b"}")
        .expect("the pack's object ends the file");
    [body, b",\"aliases\":", raw.as_bytes(), b"}"].concat()
}

#[test]
fn a_malformed_aliases_part_refuses_the_pack_naming_aliases_and_changes_nothing() {
    use serde_json::json;
    let pack = aliased(Language::Korean, &[]);
    assert!(
        !String::from_utf8_lossy(&pack.to_bytes()).contains("aliases"),
        "a pack without aliases leaves the part out (K4's file)"
    );
    let entry = |archive: &str, item: u16, keyword: &str, words: serde_json::Value| json!({"archive": archive, "item": item, "keyword": keyword, "words": words});
    let good = entry("a", 3, "name", json!(["이름", "na?e1"]));
    let read = Pack::parse(&with_part(&pack, json!([good.clone()]))).expect("a good part reads");
    assert_eq!(read.aliases((b'a', 3), b"name").map(<[_]>::len), Some(2));
    assert_eq!(
        Pack::parse(&read.to_bytes()),
        Ok(read.clone()),
        "and writes back"
    );
    let nine = "가".repeat(9);
    for bad in [
        json!([entry("a", 3, "Name", json!(["이름"]))]),
        json!([entry("a", 3, "na,me", json!(["이름"]))]),
        json!([entry("a", 3, "", json!(["이름"]))]),
        json!([entry("a", 3, "name", json!([]))]),
        json!([entry("a", 3, "name", json!(["가", "나", "다", "라", "마"]))]),
        json!([entry("a", 3, "name", json!([nine]))]),
        json!([entry("a", 3, "name", json!(["이 름"]))]),
        json!([entry("a", 3, "name", json!(["ㅇ름"]))]),
        json!([entry("a", 3, "name", json!([""]))]),
        json!([entry("c", 3, "name", json!(["이름"]))]),
        json!([entry("a", 4, "name", json!(["이름"]))]),
        json!([good.clone(), good.clone()]),
        json!([{"archive": "a", "item": 3, "keyword": "name"}]),
        json!([{"archive": "a", "item": 3, "keyword": "name", "words": ["이름"], "x": 1}]),
        json!(null),
        json!({"name": ["이름"]}),
    ] {
        let err = Pack::parse(&with_part(&pack, bad.clone())).unwrap_err();
        assert_eq!(err, PackError::Aliases, "{bad}");
        assert!(
            err.to_string().contains("aliases"),
            "the line names the part"
        );
    }
    // A field twice in an entry, as every other part of the pack refuses it (a JSON object
    // read as a map would keep the last one).
    assert!(
        Pack::parse(&with_raw_part(&pack, &good.to_string())).is_err(),
        "not a list"
    );
    assert_eq!(
        Pack::parse(&with_raw_part(&pack, &format!("[{good}]"))),
        Ok(read.clone()),
        "the raw form of the good part reads"
    );
    for twice in [
        r#"[{"archive":"a","item":3,"keyword":"name","words":[],"words":["이름"]}]"#,
        r#"[{"archive":"a","item":3,"keyword":"Name","keyword":"name","words":["이름"]}]"#,
    ] {
        let err = Pack::parse(&with_raw_part(&pack, twice)).unwrap_err();
        assert_eq!(err, PackError::Aliases, "{twice}");
        assert!(err.to_string().contains("aliases"));
    }
    // A word that holds one of the game's word delimiters refuses the pack at install, the
    // aliases part named, and the locale stays as it was (the pack installed before).
    let delimited = aliased(Language::Korean, &[("name", &["이름."])]);
    let f = talk_files(&script(BODY));
    let mut s = state(&[]);
    s.converse.text = Some(ConverseText::from_pairs(MESSAGES.iter().map(|&(m, _)| {
        let t = if m == Msg::Delimiters {
            b" .".to_vec()
        } else {
            text().get(m).to_vec()
        };
        (m, t)
    })));
    locale::install(&mut s, &f, Rc::new(read), None).expect("the good pack checks");
    let before = s.clone();
    let err = locale::install(&mut s, &f, Rc::new(delimited.clone()), None).unwrap_err();
    assert_eq!(err.part, Part::Aliases);
    assert!(err.to_string().contains("aliases"));
    assert_eq!(s, before, "nothing changed");
    // With the synthetic messages' delimiter (the space alone) it checks.
    let mut s = state(&[]);
    locale::install(&mut s, &f, Rc::new(delimited), None).expect("no delimiter in the word");
}

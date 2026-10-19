//! Slice K3 (spec 08): the pack's object names and books on synthetic files (the test's own
//! text): a round trip and a K2 pack's file unchanged, each refusal of the reader (a book over
//! the buffer among them), the check at install against `look.lzd` and `book.dat`, the units
//! (U6Lib's count, the control bytes), and at run time: a Korean name with its plural markers
//! dropped and no article, an `en` pack's English rules, logic reading the English, a miss in
//! English and counted, a book in the pack's grammar, one the pack lacks or too long in English.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use u6_formats::basetile::BaseTiles;
use u6_formats::look::{LookEntry, LookTable};
use u6_formats::tileflag::TileFlags;

use super::strings::Catalogue;
use super::texts::{self, BOOK_BYTES, ExportText, TextKind};
use super::*;
use crate::converse::disasm::catalog::hash64;
use crate::engine::{Engine, Io};
use crate::objects::commands::{GameData, ObjCommands, name_prefix, read_book};
use crate::objects::{Shown, Slot, create_on_map};

const SENTINEL: &str = "Zq8LookSentinel";

/// The test's look table: tile 0 (a name), 10 (empty: the nothing word), 40 (a name with plural
/// markers) and 41 (one with a `%` directive), then the terminator and a trailer name.
fn table() -> LookTable {
    let entry = |tile: u16, text: &str| LookEntry {
        tile,
        text: text.into(),
    };
    LookTable {
        entries: vec![
            entry(0, "zero"),
            entry(10, ""),
            entry(40, "loa/f\\ves"),
            entry(41, "50% wet"),
        ],
        trailer: vec![0x00, 0x08, b'z', 0],
    }
}

/// `table` as `look.lzd` holds it: LZW of the entries, the terminator and the trailer's text.
fn lzd(t: &LookTable) -> Vec<u8> {
    let mut d = Vec::new();
    for e in &t.entries {
        d.extend_from_slice(&e.tile.to_le_bytes());
        d.extend_from_slice(e.text.as_bytes());
        d.push(0);
    }
    d.extend_from_slice(&t.trailer);
    u6_formats::lzw::encode(&d).unwrap()
}

/// A `book.dat` of four entries: a book, an empty entry (offset 0), a sign with a `%` directive
/// and an empty text.
fn book_dat() -> Vec<u8> {
    let texts: [Option<&[u8]>; 4] = [
        Some(b"Read me, traveller.\n"),
        None,
        Some(b"Sign: 5%% off"),
        Some(b""),
    ];
    let (mut offsets, mut body) = (Vec::new(), Vec::new());
    let mut at = 8u16;
    for t in texts {
        match t {
            Some(t) => {
                offsets.push(at);
                body.extend_from_slice(t);
                body.push(0);
                at += t.len() as u16 + 1;
            }
            None => offsets.push(0),
        }
    }
    let mut d: Vec<u8> = offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
    d.extend(body);
    d
}

fn files() -> Files {
    Files::new(BTreeMap::from([
        ("U6/LOOK.LZD".to_string(), lzd(&table())),
        ("U6/BOOK.DAT".to_string(), book_dat()),
    ]))
}

fn entry(english: &[u8], text: &str) -> RunText {
    RunText {
        len: english.len() as u32,
        hash: hash64(english),
        text: text.as_bytes().to_vec(),
    }
}

/// A pack (no runs) of `look` and `books` in `language`.
fn pack(
    language: Language,
    look: impl IntoIterator<Item = (u32, RunText)>,
    books: impl IntoIterator<Item = (u32, RunText)>,
) -> Pack {
    Pack::new(language, "test/1", fingerprint([]), [])
        .unwrap()
        .with_look(look)
        .unwrap()
        .with_books(books)
        .unwrap()
}

/// The Korean pack of the tests: entries 0, 2 and 3 of the table, books 0 and 2.
fn korean() -> Pack {
    pack(
        Language::Korean,
        [
            (0, entry(b"zero", "영")),
            (2, entry(b"loa/f\\ves", "빵/s")),
            (3, entry(b"50% wet", "50% 젖은")),
        ],
        [
            (0, entry(b"Read me, traveller.\n", "그대{이/가} 읽노라.\n")),
            (2, entry(b"Sign: 5%% off", "팻말: 5%% 덜")),
        ],
    )
}

fn file(json: &str) -> Vec<u8> {
    [&b"U6LP\n"[..], json.as_bytes()].concat()
}

/// The JSON of a pack of no runs with `extra` fields after the K1 ones.
fn json_with(extra: &str) -> String {
    format!(
        "{{\"format\":\"u6.locale-pack/1\",\"language\":\"ko\",\"tool\":\"test/1\",\"base\":{{\"converse\":\"{:016x}\"}},\"converse\":[]{extra}}}",
        fingerprint([])
    )
}

/// One entry of `look` or `books` as JSON.
fn entry_json(index: u32, len: u32, text: &str) -> String {
    format!("{{\"index\":{index},\"len\":{len},\"hash\":\"0123456789abcdef\",\"text\":\"{text}\"}}")
}

#[test]
fn a_pack_with_object_names_and_books_round_trips_and_a_k2_pack_is_its_file() {
    let p = korean();
    let bytes = p.to_bytes();
    assert_eq!(Pack::parse(&bytes), Ok(p.clone()));
    assert_eq!(
        (p.look_entries(), p.books(), p.runs(), p.scripts()),
        (3, 2, 0, 0)
    );
    assert_eq!(p.look(2).map(|t| t.len), Some(9));
    assert!(p.book(2).is_some() && p.book(1).is_none());
    let text = String::from_utf8(bytes[5..].to_vec()).unwrap();
    let (look, books) = (
        text.find(",\"look\":[{\"index\":0,"),
        text.find(",\"books\":[{\"index\":0,"),
    );
    assert!(
        look.is_some() && books > look,
        "after the K2 maps, look then books"
    );
    // A pack without them writes neither: a K2 pack's file, byte for byte.
    let k2 = Pack::new(Language::Korean, "test/1", 7, [])
        .unwrap()
        .with_strings([(
            hash64(b"Naught"),
            StringText {
                len: 6,
                text: "없음".as_bytes().to_vec(),
            },
        )])
        .unwrap();
    let k2_bytes = k2.to_bytes();
    assert_eq!(
        k2_bytes,
        file(&format!(
            "{{\"format\":\"u6.locale-pack/1\",\"language\":\"ko\",\"tool\":\"test/1\",\"base\":{{\"converse\":\"0000000000000007\"}},\"converse\":[],\"strings\":[{{\"hash\":\"{:016x}\",\"len\":6,\"text\":\"없음\"}}]}}",
            hash64(b"Naught")
        ))
    );
    assert_eq!(Pack::parse(&k2_bytes), Ok(k2));
}

#[test]
fn each_broken_rule_of_the_look_and_books_maps_refuses_the_pack() {
    let look = |e: &str| json_with(&format!(",\"look\":[{e}]"));
    let books = |e: &str| json_with(&format!(",\"books\":[{e}]"));
    let near = "가".repeat((BOOK_BYTES - 1) / 3);
    let full = format!("{near}{}", "a".repeat(BOOK_BYTES - near.len()));
    assert_eq!(full.len(), BOOK_BYTES);
    let under = &full[..BOOK_BYTES - 1];
    let many: Vec<String> = (0..=texts::LOOK_ENTRIES as u32)
        .map(|i| entry_json(i, 1, "가"))
        .collect();
    for (json, want) in [
        (look(&entry_json(2048, 3, "가")), PackError::LookEntry),
        (look(&entry_json(1, 0, "가")), PackError::LookEntry),
        (look(&entry_json(1, 257, "가")), PackError::LookEntry),
        (
            look(&entry_json(1, 3, &"가".repeat(257))),
            PackError::LookEntry,
        ),
        (
            look(&format!(
                "{},{}",
                entry_json(1, 3, "가"),
                entry_json(1, 3, "나")
            )),
            PackError::Duplicate,
        ),
        (look(&entry_json(1, 3, "中")), PackError::Text),
        (
            look(&entry_json(1, 3, "가").replace("0123456789abcdef", "0123")),
            PackError::Hash,
        ),
        (look(&many.join(",")), PackError::TooManyRuns),
        (books(&entry_json(255, 3, "가")), PackError::BookEntry),
        (books(&entry_json(0, 0x2801, "가")), PackError::BookEntry),
        // Spec 08 §6: a book is under 0x2800 bytes; one of 0x2800 is refused.
        (books(&entry_json(0, 3, &full)), PackError::BookCapacity),
        (
            books(&entry_json(0, 3, &format!("{SENTINEL}\\u0001"))),
            PackError::Text,
        ),
        (
            look(&entry_json(1, 3, "가").replace("\"len\"", "\"note\":1,\"len\"")),
            PackError::Shape,
        ),
    ] {
        assert_eq!(Pack::parse(&file(&json)), Err(want), "{want:?}");
        assert!(!want.to_string().contains(SENTINEL));
    }
    let ok = Pack::parse(&file(&books(&entry_json(0, 3, under)))).unwrap();
    assert_eq!(ok.book(0).map(|t| t.text.len()), Some(BOOK_BYTES - 1));
    assert_eq!(
        PackError::BookCapacity.to_string(),
        "a book in it is longer than the game reads a book"
    );
}

#[test]
fn the_units_are_u6libs_entries_of_text_and_the_look_entries_with_one() {
    let b = book_dat();
    assert_eq!(
        texts::book_units(&b),
        [
            (0, b"Read me, traveller.\n".to_vec()),
            (2, b"Sign: 5%% off".to_vec())
        ],
        "the empty entry (offset 0) and the empty text are none"
    );
    assert!(!texts::is_book_unit(&b, 1) && !texts::is_book_unit(&b, 3));
    assert!(!texts::is_book_unit(&b, 4), "past the offset table");
    assert_eq!(texts::book_text(&b, 0), b"Read me, traveller.\n");
    assert!(texts::book_text(&b, usize::MAX).is_empty(), "quality 0");
    let mut long = b[..8].to_vec();
    long.extend(std::iter::repeat_n(b'x', BOOK_BYTES + 5));
    assert_eq!(
        texts::book_text(&long, 0).len(),
        BOOK_BYTES,
        "the buffer's bytes"
    );
    let t = table();
    let look: Vec<usize> = texts::look_units(&t).iter().map(|(i, _)| *i).collect();
    assert_eq!(look, [0, 2, 3], "the empty entry is none");
    let mut odd = t.clone();
    odd.entries[0].text = "ze\u{1}ro".into();
    assert_eq!(
        texts::look_units(&odd).len(),
        2,
        "a control byte: no unit, it stays English"
    );
}

/// Installs `pack` on an empty state over `files` (no catalogue).
fn install_on(files: &Files, pack: &Pack) -> (State, Result<(), Mismatch>) {
    let mut s = State::empty();
    let r = install_catalogued(
        &mut s,
        files,
        Rc::new(pack.clone()),
        None,
        Catalogue::default(),
    );
    (s, r)
}

#[test]
fn object_names_and_books_are_checked_against_the_files_at_install() {
    let f = files();
    let (s, r) = install_on(&f, &korean());
    assert_eq!(r, Ok(()));
    assert!(s.locale.pack.is_some() && s.ui.text.wide);
    let look_bad = Mismatch { part: Part::Look };
    let books_bad = Mismatch { part: Part::Books };
    let ko = Language::Korean;
    for (p, want) in [
        // Another English (its hash), an entry that is no unit, a `%` directive dropped.
        (pack(ko, [(0, entry(b"zerO", "영"))], []), look_bad),
        (pack(ko, [(1, entry(b"zero", "영"))], []), look_bad),
        (pack(ko, [(3, entry(b"50% wet", "50 젖은"))], []), look_bad),
        (
            pack(ko, [], [(2, entry(b"Sign: 5%% off", "팻말"))]),
            books_bad,
        ),
        (
            pack(ko, [], [(1, entry(b"Read me, traveller.\n", "가"))]),
            books_bad,
        ),
        (
            pack(ko, [], [(0, entry(b"Read me, traveller!\n", "가"))]),
            books_bad,
        ),
    ] {
        let (s, r) = install_on(&f, &p);
        assert_eq!(r, Err(want));
        assert!(
            s.locale.pack.is_none() && !s.ui.text.wide,
            "nothing changed"
        );
    }
    // A part with no file to check it against is refused; a pack without it needs none.
    let none = Files::new(BTreeMap::new());
    assert_eq!(install_on(&none, &korean()).1, Err(look_bad));
    let only_books = pack(ko, [], [(0, entry(b"Read me, traveller.\n", "가"))]);
    assert_eq!(install_on(&none, &only_books).1, Err(books_bad));
    assert_eq!(install_on(&none, &pack(ko, [], [])).1, Ok(()));
    assert_eq!(
        look_bad.to_string(),
        "its object names part does not match the game's look table"
    );
    assert_eq!(
        books_bad.to_string(),
        "its books part does not match the game's books and signs"
    );
}

/// The image addresses of the strings the tests read back, as the object commands use them: the
/// three articles, Look's blank lines before a book, the book file's name.
const S_A: u32 = 0x0012_83e2;
const S_AN: u32 = 0x0012_83e6;
const S_THE: u32 = 0x0012_83ea;
const S_BLANK_LINES: u32 = 0x0012_83f0;
const S_BOOK_DAT: u32 = 0x0012_83f4;

/// A world over the test's look table: tiles 4 × type + frame, the article on tiles 40 and 41,
/// the articles' and the book file's strings.
fn world() -> State {
    let mut s = State::empty();
    s.tables.basetile = BaseTiles {
        base: (0..1024).map(|t| t * 4).collect(),
    };
    s.tables.tileflags = TileFlags {
        flags1: vec![0; 2048],
        flags2: vec![0; 2048],
        weight: vec![0; 1024],
        flags3: vec![0; 2048],
    };
    s.tables.tileflags.flags3[40] = 0x40;
    s.tables.tileflags.flags3[41] = 0x40;
    s.tables.look = table();
    let d = &mut s.objects.defs;
    d.stackable = vec![60];
    d.nothing = "<none>".into();
    d.nothing_lower = "<none-lc>".into();
    s.objcmds = ObjCommands::from_data(GameData::with_bytes(&[
        (S_A, b"a\0"),
        (S_AN, b"n\0"),
        (S_THE, b"t\0"),
        (S_BLANK_LINES, b"\0"),
        (S_BOOK_DAT, b"BOOK.DAT\0"),
    ]));
    crate::objects::init_slots(&mut s.objects);
    s
}

/// Type 10's tiles: frame 0 is tile 40, frame 1 tile 41.
const LOAF: u16 = 10;
const WET: u16 = 10 | (1 << 10);

/// The format strings queued, as text.
fn queued(s: &State) -> Vec<String> {
    s.main
        .pending_print
        .iter()
        .filter_map(|p| match p {
            crate::main_loop::Pending::Print(f, _) => Some(String::from_utf8_lossy(f).into()),
            _ => None,
        })
        .collect()
}

fn installed(mut s: State, pack: Pack) -> State {
    install_catalogued(&mut s, &files(), Rc::new(pack), None, Catalogue::default()).unwrap();
    s
}

#[test]
fn a_korean_name_drops_its_plural_markers_and_takes_no_article_while_logic_reads_the_english() {
    let mut s = world();
    let loaf: Slot = create_on_map(&mut s.objects, LOAF, 0, 0, 3, 3, 0);
    let wet: Slot = create_on_map(&mut s.objects, WET, 0, 0, 4, 3, 0);
    let mut s = installed(s, korean());
    // Shown: the pack's, `/s` dropped (spec 08 §1, the pack's language), no article queued.
    assert_eq!(
        crate::objects::shown_name(&s, loaf),
        Shown::Pack("빵".as_bytes().to_vec())
    );
    assert_eq!(
        crate::objects::shown_name(&s, wet).into_bytes(),
        "50% 젖은".as_bytes()
    );
    assert!(crate::objects::korean_name(&s, loaf));
    assert_eq!(name_prefix(&mut s, loaf), 1);
    assert!(queued(&s).is_empty(), "no article before a Korean name");
    // Logic reads the English (a name compared, a panel's length), its markers as they are.
    assert_eq!(crate::objects::display_name(&s, loaf), "loa/f\\ves");
    assert_eq!(crate::objects::look_text(&s, 40), "loa/f\\ves");
    assert_eq!(s.locale.look.get(), [2, 0], "the two shown, counted");
    // The pack's grammar is its language's: an `en` pack's name keeps its article and leaves
    // its plural markers to the printer, as English does.
    let mut e = world();
    let loaf = create_on_map(&mut e.objects, LOAF, 0, 0, 3, 3, 0);
    let en = pack(
        Language::English,
        [(2, entry(b"loa/f\\ves", "bu/n\\ns"))],
        [],
    );
    let mut e = installed(e, en);
    assert_eq!(
        crate::objects::shown_name(&e, loaf),
        Shown::Pack(b"bu/n\\ns".to_vec())
    );
    assert!(!crate::objects::korean_name(&e, loaf));
    name_prefix(&mut e, loaf);
    assert_eq!(queued(&e), ["a"], "the article, as in English");
}

#[test]
fn a_name_the_pack_lacks_stays_english_and_is_counted_a_read_past_the_entries_is_none() {
    let only = pack(Language::Korean, [(2, entry(b"loa/f\\ves", "빵"))], []);
    let mut s = installed(world(), only);
    // Entry 0: not in the pack.
    assert_eq!(
        crate::objects::shown_look(&s, 0),
        Shown::English("zero".into())
    );
    assert!(!crate::objects::korean_look(&s, 0));
    assert_eq!(s.locale.look.get(), [0, 1], "a miss");
    // The empty entry (the nothing word) and a read past the entries: no unit, not counted.
    assert_eq!(
        crate::objects::shown_look(&s, 5),
        Shown::English("<none-lc>".into())
    );
    assert_eq!(
        crate::objects::shown_look(&s, 60),
        Shown::English("z".into())
    );
    assert_eq!(s.locale.look.get(), [0, 1]);
    // The table's bytes not the ones the pack was made from: English, a miss.
    s.tables.look.entries[2].text = "loa/f\\vez".into();
    assert_eq!(
        crate::objects::shown_look(&s, 40),
        Shown::English("loa/f\\vez".into())
    );
    assert_eq!(s.locale.look.get(), [0, 2]);
    // No pack: English, nothing counted.
    let plain = world();
    assert_eq!(
        crate::objects::shown_look(&plain, 40),
        Shown::English("loa/f\\ves".into())
    );
    assert_eq!(plain.locale.look.get(), [0, 0]);
}

/// Reads the book of quality `q` (Look's 0x1292d0) on a text layer, with `pack` installed (or
/// none): the scroll after it and the book counts.
fn read(q: u8, pack: Option<Pack>) -> (String, [u32; 2]) {
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    let book = create_on_map(&mut s.objects, LOAF, 0, u16::from(q) << 8, 3, 3, 0);
    let s = match pack {
        Some(p) => installed(s, p),
        None => s,
    };
    let mut e = Engine::with_game(
        s,
        files(),
        move |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
            read_book(&st, &io, book).await;
        },
    );
    for _ in 0..50 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished(), "the read ended");
    let s = e.state();
    let t = &s.ui.text;
    let cell = |c: &u16| crate::ui::wide::cell_char(*c);
    let mut lines = s.ui.messages.clone();
    let mut last: String = t.transcript_line.iter().map(cell).collect();
    last.extend(
        t.line[usize::from(t.line_start)..usize::from(t.line_len)]
            .iter()
            .map(cell),
    );
    lines.push(last);
    (lines.join("\n"), s.locale.books.get())
}

#[test]
fn a_book_from_the_pack_is_read_in_its_grammar_and_one_it_lacks_or_too_long_stays_english() {
    let (english, counts) = read(1, None);
    assert!(english.contains("Read me, traveller."));
    assert_eq!(counts, [0, 0]);
    // The pack's book, its josa marker resolved in its grammar (after 대, a vowel: 가).
    let (korean_book, counts) = read(1, Some(korean()));
    assert!(korean_book.contains("그대가 읽노라."), "the pack's book");
    assert!(!korean_book.contains("Read me"));
    assert_eq!(counts, [1, 0]);
    // The sign, printed as a format: its `%%` one `%`.
    assert!(read(3, Some(korean())).0.contains("팻말: 5% 덜"));
    // A book the pack lacks: English, a miss.
    let lacking = pack(
        Language::Korean,
        [],
        [(2, entry(b"Sign: 5%% off", "팻말: 5%% 덜"))],
    );
    let (shown, counts) = read(1, Some(lacking));
    assert!(shown.contains("Read me, traveller."));
    assert_eq!(counts, [0, 1]);
    // Quality 2 names the empty entry: no unit, nothing counted.
    assert_eq!(read(2, Some(korean())).1, [0, 0]);
    // A text the reader refuses, put in past it: at the read too, English and a miss.
    let mut long = korean();
    long.books.insert(
        0,
        entry(b"Read me, traveller.\n", &"가".repeat(BOOK_BYTES / 3 + 1)),
    );
    let (shown, counts) = read(1, Some(long));
    assert!(shown.contains("Read me, traveller."));
    assert_eq!(counts, [0, 1]);
}

#[test]
fn the_export_lists_the_texts_by_kind_and_reads_back_and_a_k2_export_is_unchanged() {
    let texts = texts::export(&files()).unwrap();
    let kinds: Vec<(TextKind, u32)> = texts.iter().map(|t| (t.kind, t.index)).collect();
    assert_eq!(
        kinds,
        [
            (TextKind::Look, 0),
            (TextKind::Look, 2),
            (TextKind::Look, 3),
            (TextKind::Book, 0),
            (TextKind::Book, 2)
        ]
    );
    let mut e = Catalogue::default().export();
    assert!(!e.to_json().contains("texts"), "no texts: K2's export");
    e.texts = texts;
    let json = e.to_json();
    assert!(json.contains(r#""texts":[{"kind":"look","index":0,"hash":""#));
    assert_eq!(strings::Export::from_json(&json), Ok(e.clone()));
    // A text whose text and hash disagree, or keyed where no pack keys one, is refused with a
    // fixed sentence.
    let mut bad = e.clone();
    bad.texts[1].text.push('x');
    assert_eq!(
        strings::Export::from_json(&bad.to_json()),
        Err("a text's index, text, length and hash disagree")
    );
    let mut far = e;
    far.texts[4] = ExportText {
        index: texts::BOOK_INDICES as u32,
        ..far.texts[4].clone()
    };
    assert!(strings::Export::from_json(&far.to_json()).is_err());
    // A look.lzd that does not read fails, named only.
    let broken = Files::new(BTreeMap::from([(
        "LOOK.LZD".to_string(),
        SENTINEL.as_bytes().to_vec(),
    )]));
    assert_eq!(texts::export(&broken), Err("look.lzd does not read"));
    assert!(strings::reads("U6/look.lzd") && strings::reads("BOOK.DAT"));
}

//! The NPC bundles (spec 09 Design 5) on synthetic scripts built here (no game bytes): the
//! exact bundle of a conversation (name, look, greeting, turns, topics, what they require, the
//! four segment kinds), each text segment's key against 08's catalog, split runs, the input
//! ops, the `if` rules, the actors and their numbers from a synthetic objlist, the export of
//! two archives, its errors, the JSON form, and a sentinel that never reaches an error.

use std::collections::BTreeMap;

use super::bundle::{
    self, Bundle, Clause, ExportError, IfForm, IfSegment, InputKind, InputOp, Member, Narration,
    OpSegment, Portrait, Segment, Speech, TableArg, TableString, TextSegment, Tok, Topic, Traits,
    Turn, VarFrom, VarSegment, coverage, export, from_json, to_json,
};
use super::catalog::{ScriptUnits, hash64};
use super::{Kind, disassemble};
use crate::files::Files;
use crate::saveload::objlist::OBJLIST_LEN;

/// `ff 05 "Nm" f1 "look" f2`, then the body from [`BODY`].
const HEAD: &[u8] = b"\xff\x05Nm\xf1look\xf2";
const BODY: u32 = 10;
/// A string no bundle error may show.
const SENTINEL: &str = "Zq7Sentinel";

/// A script body under construction: each part's offset in the script as it is put.
#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn at(&self) -> u32 {
        BODY + self.0.len() as u32
    }

    fn put(&mut self, b: &[u8]) -> u32 {
        let at = self.at();
        self.0.extend_from_slice(b);
        at
    }

    fn jump(&mut self, to: u32) -> u32 {
        self.put(&[&[0xb0][..], &to.to_le_bytes()].concat())
    }

    /// Pads with zeros up to script offset `to`.
    fn pad(&mut self, to: u32) {
        assert!(self.at() <= to, "the body fits before {to}");
        self.0.resize((to - BODY) as usize, 0);
    }

    fn script(&self) -> Vec<u8> {
        [HEAD, &self.0].concat()
    }
}

fn text(item: u16, at: u32, t: &str) -> Segment {
    Segment::Text(TextSegment {
        key: ('a', item, at),
        text: t.into(),
        part_of: None,
    })
}

fn op(at: u32, name: &str) -> OpSegment {
    OpSegment {
        at,
        op: name.into(),
        args: Vec::new(),
        target: None,
        var: None,
        src: None,
        table: None,
    }
}

fn jump(at: u32, to: u32) -> Segment {
    Segment::Op(OpSegment {
        target: Some(to),
        ..op(at, "jump")
    })
}

fn iff(at: u32, form: IfForm, cond: Option<Vec<Tok>>) -> Segment {
    Segment::If(IfSegment { at, form, cond })
}

fn ask(at: u32) -> InputOp {
    InputOp {
        at,
        kind: InputKind::Ask,
        keys: None,
        slot: None,
        var: None,
    }
}

fn topic(at: u32, keywords: &[&str], requires: Vec<Clause>, answer: Vec<Segment>) -> Topic {
    Topic {
        at,
        keywords: keywords.iter().map(|k| k.to_string()).collect(),
        requires,
        answer,
    }
}

fn units(item: u16, s: &[u8]) -> ScriptUnits {
    let d = disassemble(b'a', item, s).unwrap_or_else(|r| panic!("refused: {r}"));
    ScriptUnits::units(&d, s)
}

/// The bundle of `s` as item `item` of `converse.a`, with no actors, and its catalog.
fn build(item: u16, s: &[u8]) -> (Bundle, ScriptUnits) {
    (Bundle::build(b'a', item, s, &[]), units(item, s))
}

/// The conversation of [`a_conversation_bundles_its_name_look_greeting_turns_and_topics`]:
/// a greeting with a slot print, an ask, a two-keyword block, an `if` / `else` pair of
/// blocks, a `*` block printing a table string, the end and the table.
fn conversation() -> (Vec<u8>, Bundle) {
    let mut b = Body::default();
    let hi = b.put(b"Hi! ");
    let slot = b.put(&[0xb5, 0x00, 0x17, 0xb3, 0xa7]);
    let ask_at = b.put(&[0xf7]);
    let kw1 = b.put(b"\xefname,job\xf6");
    let am = b.put(b"I am $N.");
    let j1 = b.jump(ask_at);
    b.put(&[0xee]);
    let if1 = b.put(&[0xa1, 0x01, 0xa7]);
    let kw2 = b.put(b"\xefwork\xf6");
    let busy = b.put(b"Busy.");
    let j2 = b.jump(ask_at);
    b.put(&[0xee]);
    let else1 = b.put(&[0xa3]);
    let kw3 = b.put(b"\xefrest\xf6");
    let idle = b.put(b"Idle.");
    let j3 = b.jump(ask_at);
    b.put(&[0xee]);
    let endif = b.put(&[0xa2]);
    let kw4 = b.put(b"\xef*\xf6");
    let what = b.put(b"What? ");
    let table = BODY + 120;
    let print = b.put(&[&[0xb5, 0xd2][..], &table.to_le_bytes(), &[0xb1]].concat());
    let j4 = b.jump(ask_at);
    b.put(&[0xee]);
    let end = b.put(&[0xff]);
    b.pad(table);
    b.put(b"first\0second\0");
    let s = b.script();
    let unless = |c: u32| Clause::Unless(vec![vec![Tok::Num(c)]]);
    let expected = Bundle {
        format: bundle::FORMAT.into(),
        archive: 'a',
        item: 9,
        hash: hash64(&s),
        refused: None,
        actors: Vec::new(),
        portraits: Vec::new(),
        name: Some("Nm".into()),
        locale: "en".into(),
        narration: Narration {
            look: vec![text(9, 5, "look")],
        },
        speech: Speech {
            greeting: vec![
                text(9, hi, "Hi! "),
                Segment::Var(VarSegment {
                    at: slot,
                    from: VarFrom::Slot,
                    slot: Some(0x17),
                    table: None,
                }),
            ],
        },
        turns: vec![Turn {
            input: Some(ask(ask_at)),
            segments: vec![
                iff(if1, IfForm::If, Some(vec![Tok::Num(1)])),
                iff(else1, IfForm::Else, None),
                iff(endif, IfForm::Endif, None),
                Segment::Op(op(end, "end")),
            ],
            topics: vec![
                topic(
                    kw1,
                    &["name", "job"],
                    vec![],
                    vec![text(9, am, "I am $N."), jump(j1, ask_at)],
                ),
                topic(
                    kw2,
                    &["work"],
                    vec![Clause::If(vec![Tok::Num(1)])],
                    vec![text(9, busy, "Busy."), jump(j2, ask_at)],
                ),
                topic(
                    kw3,
                    &["rest"],
                    vec![unless(1)],
                    vec![text(9, idle, "Idle."), jump(j3, ask_at)],
                ),
                topic(
                    kw4,
                    &["*"],
                    vec![],
                    vec![
                        text(9, what, "What? "),
                        Segment::Var(VarSegment {
                            at: print,
                            from: VarFrom::Table,
                            slot: None,
                            table: Some(TableArg {
                                addr: table,
                                index: None,
                                strings: vec![('a', 9, table)],
                            }),
                        }),
                        jump(j4, ask_at),
                    ],
                ),
            ],
        }],
        traits: Vec::new(),
        strings: vec![
            TableString {
                key: ('a', 9, table),
                table,
                index: 0,
                text: "first".into(),
                slots: Vec::new(),
                capacity: None,
                inner: Vec::new(),
            },
            TableString {
                key: ('a', 9, table + 6),
                table,
                index: 1,
                text: "second".into(),
                slots: Vec::new(),
                capacity: None,
                inner: Vec::new(),
            },
        ],
    };
    (s, expected)
}

#[test]
fn a_conversation_bundles_its_name_look_greeting_turns_and_topics() {
    let (s, expected) = conversation();
    let (b, u) = build(9, &s);
    assert_eq!(b, expected);
    let c = coverage(&b, &u);
    assert_eq!((c.runs, c.texts, c.gaps()), (6, 6, 0), "{c:?}");
}

/// Every text segment is one run of 08's catalog, at the catalog's key (archive, item, run
/// offset), with the catalog's text; every run is placed once. The key is the run's own: the
/// item and archive given, the offset where the catalog has the run.
#[test]
fn each_text_segment_is_one_catalog_run_at_its_key() {
    let (s, _) = conversation();
    for item in [3u16, 9, 200] {
        let b = Bundle::build(b'b', item, &s, &[]);
        let d = disassemble(b'b', item, &s).unwrap();
        let u = ScriptUnits::units(&d, &s);
        let texts: Vec<(char, u16, u32, String)> = b
            .segments()
            .filter_map(|g| match g {
                Segment::Text(t) => Some((t.key.0, t.key.1, t.key.2, t.text.clone())),
                _ => None,
            })
            .collect();
        let runs: Vec<(char, u16, u32, String)> = u
            .runs
            .iter()
            .map(|r| ('b', item, r.unit.offset, r.unit.text.clone()))
            .collect();
        let mut sorted = texts.clone();
        sorted.sort();
        assert_eq!(sorted, runs, "item {item}");
        for (_, _, at, t) in &texts {
            let it = d.items.iter().find(|i| i.offset == *at).unwrap();
            assert!(matches!(it.kind, Kind::Text(_)), "a run at its key");
            assert_eq!(it.span().bytes(&s), t.as_bytes(), "its bytes");
        }
        assert_eq!(coverage(&b, &u).gaps(), 0);
    }
    // A segment moved off its run, or given another's text, is a gap.
    let (s, mut b) = conversation();
    let u = units(9, &s);
    let Segment::Text(t) = &mut b.speech.greeting[0] else {
        panic!()
    };
    t.key.2 += 1;
    let c = coverage(&b, &u);
    assert_eq!((c.missing, c.unresolved), (1, 1));
    let (_, mut b) = conversation();
    let Segment::Text(t) = &mut b.narration.look[0] else {
        panic!()
    };
    t.text.push('!');
    assert_eq!(coverage(&b, &u).unresolved, 1);
    let (_, mut b) = conversation();
    b.strings.pop();
    assert_eq!(coverage(&b, &u).strings_missing, 1);
    let (_, mut b) = conversation();
    let first = b.speech.greeting[0].clone();
    b.speech.greeting.push(first);
    assert_eq!(coverage(&b, &u).repeated, 1);
}

#[test]
fn a_split_run_keeps_its_parts_linked() {
    // The block jumps into the middle of "Hi there": two runs, one sentence.
    let mut b = Body::default();
    b.put(b"Hi there");
    let ask_at = b.put(&[0xf7]);
    let kw = b.put(b"\xef*\xf6");
    let j = b.jump(BODY + 3);
    b.put(&[0xee]);
    let end = b.put(&[0xff]);
    let s = b.script();
    let (bundle, u) = build(4, &s);
    let part = |at: u32, t: &str| {
        Segment::Text(TextSegment {
            key: ('a', 4, at),
            text: t.into(),
            part_of: Some(BODY),
        })
    };
    assert_eq!(
        bundle.speech.greeting,
        [part(BODY, "Hi "), part(BODY + 3, "there")]
    );
    assert_eq!(
        bundle.turns,
        [Turn {
            input: Some(ask(ask_at)),
            segments: vec![Segment::Op(op(end, "end"))],
            topics: vec![topic(kw, &["*"], vec![], vec![jump(j, BODY + 3)])],
        }]
    );
    assert_eq!(coverage(&bundle, &u).gaps(), 0);
}

#[test]
fn every_input_op_starts_a_turn() {
    let mut b = Body::default();
    let keys = b.put(b"\xf8yn");
    let y = b.put(b"\xefy\xf6");
    let yes = b.put(b"Yes.");
    b.put(&[0xee]);
    let n = b.put(b"\xefn\xf6");
    let no = b.put(b"No.");
    b.put(&[0xee]);
    let line = b.put(&[0xf9, 0x05, 0xb3]);
    let number = b.put(&[0xfb, 0x06, 0xb2]);
    let digit = b.put(&[0xfc, 0x07, 0xb2]);
    let key = b.put(&[0xfa]);
    let end = b.put(&[0xff]);
    let s = b.script();
    let (bundle, u) = build(2, &s);
    let input = |at, kind, keys: Option<&str>, slot, var| InputOp {
        at,
        kind,
        keys: keys.map(Into::into),
        slot,
        var,
    };
    let turn = |input, topics, segments| Turn {
        input: Some(input),
        segments,
        topics,
    };
    assert_eq!(
        bundle.turns,
        [
            turn(
                input(keys, InputKind::Keys, Some("yn"), None, None),
                vec![
                    topic(y, &["y"], vec![], vec![text(2, yes, "Yes.")]),
                    topic(n, &["n"], vec![], vec![text(2, no, "No.")]),
                ],
                vec![],
            ),
            turn(
                input(line, InputKind::Line, None, Some(5), None),
                vec![],
                vec![]
            ),
            turn(
                input(number, InputKind::Number, None, None, Some(6)),
                vec![],
                vec![]
            ),
            turn(
                input(digit, InputKind::Digit, None, None, Some(7)),
                vec![],
                vec![]
            ),
            turn(
                input(key, InputKind::Key, None, None, None),
                vec![],
                vec![Segment::Op(op(end, "end"))]
            ),
        ]
    );
    assert!(bundle.speech.greeting.is_empty());
    assert_eq!(coverage(&bundle, &u).gaps(), 0);
}

/// What a topic requires follows the interpreter's skip loops: an `a2` ends every condition,
/// and after an `a3` the conditions since the last `a3` (or `a2`) do not all hold.
#[test]
fn a_topic_requires_what_the_skip_loops_make_it_need() {
    let mut b = Body::default();
    let ask_at = b.put(&[0xf7]);
    let i1 = b.put(&[0xa1, 0x01, 0xa7]);
    let i2 = b.put(&[0xa1, 0x02, 0xa7]);
    let x = b.put(b"\xefx\xf6");
    let xt = b.put(b"X");
    let jx = b.jump(ask_at);
    b.put(&[0xee]);
    let e1 = b.put(&[0xa3]);
    let y = b.put(b"\xefy\xf6");
    let yt = b.put(b"Y");
    let jy = b.jump(ask_at);
    b.put(&[0xee]);
    let i3 = b.put(&[0xa1, 0x03, 0xa7]);
    let e2 = b.put(&[0xa3]);
    let w = b.put(b"\xefw\xf6");
    let wt = b.put(b"W");
    let jw = b.jump(ask_at);
    b.put(&[0xee]);
    let end_if = b.put(&[0xa2]);
    let z = b.put(b"\xefz\xf6");
    let zt = b.put(b"Z");
    let jz = b.jump(ask_at);
    b.put(&[0xee]);
    b.put(&[0xff]);
    let s = b.script();
    let (bundle, u) = build(1, &s);
    let c = |v: u32| vec![Tok::Num(v)];
    let t = &bundle.turns[0];
    let req: Vec<(&[String], &[Clause])> = t
        .topics
        .iter()
        .map(|p| (&p.keywords[..], &p.requires[..]))
        .collect();
    let kw = |k: &str| vec![k.to_string()];
    assert_eq!(
        req,
        [
            (&kw("x")[..], &[Clause::If(c(1)), Clause::If(c(2))][..]),
            (&kw("y")[..], &[Clause::Unless(vec![c(1), c(2)])][..]),
            (
                &kw("w")[..],
                &[Clause::Unless(vec![c(1), c(2)]), Clause::Unless(vec![c(3)])][..]
            ),
            (&kw("z")[..], &[][..]),
        ]
    );
    assert_eq!(
        t.topics.iter().map(|p| p.at).collect::<Vec<_>>(),
        [x, y, w, z]
    );
    assert_eq!(
        t.topics
            .iter()
            .map(|p| p.answer.clone())
            .collect::<Vec<_>>(),
        [
            vec![text(1, xt, "X"), jump(jx, ask_at)],
            vec![text(1, yt, "Y"), jump(jy, ask_at)],
            vec![text(1, wt, "W"), jump(jw, ask_at)],
            vec![text(1, zt, "Z"), jump(jz, ask_at)],
        ]
    );
    let ifs: Vec<(u32, IfForm)> = t
        .segments
        .iter()
        .filter_map(|g| match g {
            Segment::If(i) => Some((i.at, i.form)),
            _ => None,
        })
        .collect();
    assert_eq!(
        ifs,
        [
            (i1, IfForm::If),
            (i2, IfForm::If),
            (e1, IfForm::Else),
            (i3, IfForm::If),
            (e2, IfForm::Else),
            (end_if, IfForm::Endif)
        ]
    );
    assert_eq!(coverage(&bundle, &u).gaps(), 0);
}

#[test]
fn ops_carry_their_operands_and_assigns_their_slots_and_tables() {
    let mut b = Body::default();
    let table = BODY + 80;
    // a6 03 b2 a8 05 a7: var 3 = 5; a6 04 b3 a8 d2 t 01 a7: string 1 into slot 4;
    // a6 06 b3 a8 07 b3: slot 7 into slot 6; c4 0a a7: add karma 10; b5 d5; b6.
    let int = b.put(&[0xa6, 0x03, 0xb2, 0xa8, 0x05, 0xa7]);
    let copy = b.put(
        &[
            &[0xa6, 0x04, 0xb3, 0xa8, 0xd2][..],
            &table.to_le_bytes(),
            &[0x01, 0xa7],
        ]
        .concat(),
    );
    let slot = b.put(&[0xa6, 0x06, 0xb3, 0xa8, 0x07, 0xb3]);
    let karma = b.put(&[0xc4, 0x0a, 0x0c, 0x90, 0xa7]);
    let nothing = b.put(&[0xb5, 0xd5]);
    let bye = b.put(&[0xb6]);
    b.pad(table);
    b.put(b"one\0two\0");
    let s = b.script();
    let (bundle, u) = build(6, &s);
    assert_eq!(
        bundle.speech.greeting,
        [
            Segment::Op(OpSegment {
                var: Some(3),
                args: vec![vec![Tok::Num(5)]],
                ..op(int, "assign")
            }),
            Segment::Op(OpSegment {
                var: Some(4),
                table: Some(TableArg {
                    addr: table,
                    index: Some(vec![Tok::Num(1)]),
                    strings: vec![('a', 6, table + 4)],
                }),
                ..op(copy, "assign")
            }),
            Segment::Op(OpSegment {
                var: Some(6),
                src: Some(7),
                ..op(slot, "assign")
            }),
            Segment::Op(OpSegment {
                args: vec![vec![Tok::Num(10), Tok::Num(12), Tok::Op("add".into())]],
                ..op(karma, "add_karma")
            }),
            Segment::Op(op(nothing, "print_nothing")),
            Segment::Op(op(bye, "bye")),
        ]
    );
    assert!(bundle.turns.is_empty());
    assert_eq!(coverage(&bundle, &u).gaps(), 0);
    // Spec 08 (slice K2): the copied string carries its slot and capacity; the other none, and
    // its JSON has no such field (a bundle without them reads as before).
    let strings: Vec<_> = bundle
        .strings
        .iter()
        .map(|t| (t.key.2, t.slots.clone(), t.capacity))
        .collect();
    assert_eq!(
        strings,
        [
            (table, vec![], None),
            (
                table + 4,
                vec![4],
                Some(super::catalog::SLOT_BUFFER_CAPACITY)
            )
        ]
    );
    let json = super::bundle::to_json(std::slice::from_ref(&bundle));
    assert_eq!(json.matches("\"slots\"").count(), 1);
    assert_eq!(json.matches("\"capacity\"").count(), 1);
    assert!(!json.contains("\"inner\""));
    assert_eq!(super::bundle::from_json(&json).unwrap(), vec![bundle]);
}

#[test]
fn a_refused_script_is_a_bundle_with_its_reason_and_nothing_else() {
    let s = [HEAD, &[0x80, 0xb6]].concat();
    let b = Bundle::build(b'b', 12, &s, &[]);
    assert_eq!(b.refused.as_deref(), Some("unknown op 0x80 at 0x000a"));
    assert_eq!((b.archive, b.item, b.hash), ('b', 12, hash64(&s)));
    assert!(b.name.is_none() && b.segments().next().is_none() && b.turns.is_empty());
    assert!(b.strings.is_empty());
    let u = ScriptUnits::build(b'b', 12, &s);
    assert_eq!(coverage(&b, &u).gaps(), 0);
}

/// The objlist bytes of a synthetic new game: actor 5 a type 0x187 with worktype 7 and
/// alignment 2, actor 0xe5 a type 0x175 (it talks with `converse.b` item 0x66), the hero's
/// portrait 3.
fn objlist() -> Vec<u8> {
    let mut f = vec![0u8; OBJLIST_LEN];
    let mut type_frame =
        |a: usize, t: u16| f[0x400 + 2 * a..0x402 + 2 * a].copy_from_slice(&t.to_be_bytes());
    type_frame(5, 0x187);
    type_frame(0xe5, 0x175);
    f[0x0800 + 5] = 2 << 5;
    f[0x11f1 + 5] = 7;
    // The objlist's tail starts at 0x1bf1; the hero's portrait byte is its 0x81st.
    f[0x1bf1 + 0x81] = 3;
    f
}

fn lib(items: &[Option<Vec<u8>>]) -> Vec<u8> {
    super::super::tests::lib(items)
}

fn raw(s: &[u8]) -> Vec<u8> {
    [&[0, 0, 0, 0][..], s].concat()
}

fn files(entries: &[(&str, Vec<u8>)]) -> Files {
    Files::new(
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

#[test]
fn the_export_bundles_every_script_of_both_archives_with_the_actors_that_use_it() {
    let (s, _) = conversation();
    let a: Vec<Option<Vec<u8>>> = (0..7)
        .map(|i| [1, 5].contains(&i).then(|| raw(&s)))
        .collect();
    let mut b: Vec<Option<Vec<u8>>> = vec![None; 0x67];
    b[0x66] = Some(raw(&s));
    let f = files(&[
        ("U6/CONVERSE.A", lib(&a)),
        ("U6/converse.b", lib(&b)),
        ("U6/savegame/objlist", objlist()),
        ("U6/other", vec![1, 2, 3]),
    ]);
    let out = export(&f).unwrap();
    let keys: Vec<(char, u16)> = out.iter().map(|b| (b.archive, b.item)).collect();
    assert_eq!(keys, [('a', 1), ('a', 5), ('b', 0x66)]);
    let who: Vec<(&[u16], &[Portrait], &[Traits])> = out
        .iter()
        .map(|b| (&b.actors[..], &b.portraits[..], &b.traits[..]))
        .collect();
    let portrait = |actor, archive, item| Portrait {
        actor,
        archive,
        item,
    };
    let traits = |actor, obj_type, worktype, alignment| Traits {
        actor,
        obj_type,
        worktype,
        alignment,
    };
    assert_eq!(
        who,
        [
            (
                &[1][..],
                &[portrait(1, 'z', 2)][..],
                &[traits(1, 0, 0, 0)][..]
            ),
            (
                &[5][..],
                &[portrait(5, 'a', 4)][..],
                &[traits(5, 0x187, 7, 2)][..]
            ),
            (
                &[0xc9, 0xe5][..],
                &[portrait(0xc9, 'b', 0x66), portrait(0xe5, 'b', 0x5d)][..],
                &[traits(0xc9, 0, 0, 0), traits(0xe5, 0x175, 0, 0)][..]
            ),
        ]
    );
    // The same script gives the same bundle wherever it is, apart from its key and actors.
    let (_, mut expected) = conversation();
    for b in &out {
        let mut b = b.clone();
        assert_eq!(b.hash, expected.hash);
        b.actors.clear();
        b.portraits.clear();
        b.traits.clear();
        expected.archive = b.archive;
        expected.item = b.item;
        let rekey = |k: &mut (char, u16, u32)| (k.0, k.1) = (b.archive, b.item);
        let mut e = expected.clone();
        for g in e
            .narration
            .look
            .iter_mut()
            .chain(&mut e.speech.greeting)
            .chain(e.turns.iter_mut().flat_map(|t| {
                t.segments
                    .iter_mut()
                    .chain(t.topics.iter_mut().flat_map(|p| &mut p.answer))
            }))
        {
            match g {
                Segment::Text(t) => rekey(&mut t.key),
                Segment::Var(VarSegment { table: Some(t), .. }) => {
                    t.strings.iter_mut().for_each(rekey)
                }
                _ => {}
            }
        }
        e.strings.iter_mut().for_each(|s| rekey(&mut s.key));
        assert_eq!(b, e);
    }
    // Without an objlist: the actor slots' own mapping, no traits, no slot from 0xe0.
    let f = files(&[("converse.b", lib(&b))]);
    let out = export(&f).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(
        (
            &out[0].actors[..],
            &out[0].portraits[..],
            out[0].traits.is_empty()
        ),
        (&[0xc9][..], &[portrait(0xc9, 'b', 0x66)][..], true)
    );
}

#[test]
fn the_cast_maps_every_actor_slot_as_the_engine_does() {
    let mut state = crate::state::State::empty();
    crate::saveload::objlist::decode(&mut state, &objlist());
    let cast = bundle::cast(&state, true);
    let actors = |k: (u8, u16)| -> Vec<u16> { cast[&k].iter().map(|m: &Member| m.actor).collect() };
    assert_eq!(actors((b'a', 1)), [1]);
    assert_eq!(actors((b'a', 0x62)), [0x62]);
    assert_eq!(actors((b'b', 0)), [0x63]);
    assert_eq!(actors((b'b', 0x7c)), [0xdf]);
    assert_eq!(actors((b'b', 0x66)), [0xc9, 0xe5]);
    assert!(!cast.contains_key(&(b'a', 0)), "actor 0 never talks");
    let all: usize = cast.values().map(Vec::len).sum();
    assert_eq!(
        all,
        0xdf + 1,
        "slots 1..0xdf and the one talking type from 0xe0"
    );
}

#[test]
fn export_errors_name_only_what_failed() {
    let s = [HEAD, SENTINEL.as_bytes(), &[0xff]].concat();
    let none = files(&[(SENTINEL, raw(&s))]);
    assert_eq!(export(&none), Err(ExportError::NoArchive));
    let bad = files(&[
        ("converse.a", lib(&[Some(vec![1, 2, 3, 4, 5, 6])])),
        ("converse.b", lib(&[Some(raw(&s))])),
    ]);
    assert_eq!(export(&bad), Err(ExportError::Unreadable(b'a')));
    for e in [ExportError::NoArchive, ExportError::Unreadable(b'a')] {
        let m = format!("{e} {e:?}");
        assert!(!m.contains(SENTINEL));
    }
    assert_eq!(
        ExportError::Unreadable(b'b').to_string(),
        "converse.b does not read"
    );
    assert!(bundle::reads("U6/savegame/OBJLIST") && bundle::reads("a\\Converse.B"));
    assert!(!bundle::reads("U6/converse.c") && !bundle::reads("objlist.bak"));
}

/// The JSON form: the field names and segment kinds as documented, a bundle reads back to
/// itself, another format is refused, and an error never quotes the input.
#[test]
fn the_json_form_reads_back_and_names_only_places_in_errors() {
    let mut b = Body::default();
    let hi = b.put(b"Hi.");
    let ask_at = b.put(&[0xf7]);
    let kw = b.put(b"\xefbye\xf6");
    let bye = b.put(&[0xb6]);
    b.put(&[0xee]);
    let end = b.put(&[0xff]);
    let s = b.script();
    let members = [Member {
        actor: 9,
        portrait: Some((b'a', 8)),
        traits: Some(Traits {
            actor: 9,
            obj_type: 0x187,
            worktype: 2,
            alignment: 1,
        }),
    }];
    let one = Bundle::build(b'a', 9, &s, &members);
    let json = to_json(std::slice::from_ref(&one));
    let h = format!("{:016x}", hash64(&s));
    assert_eq!(
        json,
        format!(
            "[{{\"format\":\"u6.npc-bundle/1\",\"archive\":\"a\",\"item\":9,\"hash\":\"{h}\",\
             \"refused\":null,\"actors\":[9],\"portraits\":[{{\"actor\":9,\"archive\":\"a\",\
             \"item\":8}}],\"name\":\"Nm\",\"locale\":\"en\",\"narration\":{{\"look\":[\
             {{\"kind\":\"text\",\"key\":[\"a\",9,5],\"text\":\"look\"}}]}},\"speech\":{{\
             \"greeting\":[{{\"kind\":\"text\",\"key\":[\"a\",9,{hi}],\"text\":\"Hi.\"}}]}},\
             \"turns\":[{{\"input\":{{\"at\":{ask_at},\"kind\":\"ask\"}},\"segments\":[\
             {{\"kind\":\"op\",\"at\":{end},\"op\":\"end\"}}],\
             \"topics\":[{{\"at\":{kw},\"keywords\":[\"bye\"],\"requires\":[],\"answer\":[\
             {{\"kind\":\"op\",\"at\":{bye},\"op\":\"bye\"}}]}}]}}],\"traits\":[{{\"actor\":9,\
             \"type\":391,\"worktype\":2,\"alignment\":1}}],\"strings\":[]}}]"
        )
    );
    let (_, conv) = conversation();
    let all = vec![one, conv];
    assert_eq!(from_json(&to_json(&all)).unwrap(), all);
    let mut other = all.clone();
    other[1].format = "u6.npc-bundle/0".into();
    assert_eq!(
        from_json(&to_json(&other)),
        Err("bundle 1: not u6.npc-bundle/1".into())
    );
    let broken = to_json(&all).replace("\"locale\":\"en\"", &format!("\"locale\":{SENTINEL}"));
    let e = from_json(&broken).unwrap_err();
    assert!(
        e.starts_with("not a bundle array (line 1, column ") && !e.contains(SENTINEL),
        "{e}"
    );
    let iff_json = serde_json::to_string(&iff(
        4,
        IfForm::If,
        Some(vec![Tok::Num(1), Tok::Op("not".into())]),
    ))
    .unwrap();
    assert_eq!(
        iff_json,
        "{\"kind\":\"if\",\"at\":4,\"form\":\"if\",\"cond\":[1,\"not\"]}"
    );
    let unless = serde_json::to_string(&Clause::Unless(vec![vec![Tok::Num(2)]])).unwrap();
    assert_eq!(unless, "{\"unless\":[[2]]}");
}

/// The budget counts what a bundle takes: a script within it builds whole and spends exactly
/// that much; one item less refuses it with the fixed reason, its actors kept and nothing else
/// held, and its catalog runs counted as explained, not as gaps.
#[test]
fn a_bundle_over_its_budget_is_refused_at_the_item_past_it() {
    let (s, expected) = conversation();
    let members = [Member {
        actor: 9,
        portrait: Some((b'a', 8)),
        traits: None,
    }];
    let (whole, n) = Bundle::build_within(b'a', 9, &s, &members, usize::MAX);
    assert!(n > 0);
    assert_eq!(
        Bundle::build_within(b'a', 9, &s, &members, n),
        (whole.clone(), n)
    );
    let (cut, spent) = Bundle::build_within(b'a', 9, &s, &members, n - 1);
    assert_eq!(spent, n - 1);
    assert_eq!(cut.refused.as_deref(), Some(bundle::OVER_BUDGET));
    assert_eq!(bundle::OVER_BUDGET, "over the bundle budget");
    assert_eq!((cut.archive, cut.item, cut.hash), ('a', 9, expected.hash));
    assert_eq!(
        (&cut.actors, &cut.portraits),
        (&whole.actors, &whole.portraits)
    );
    assert!(cut.name.is_none() && cut.segments().next().is_none() && cut.turns.is_empty());
    assert!(cut.strings.is_empty());
    let c = coverage(&cut, &units(9, &s));
    assert_eq!((c.explained, c.gaps()), (6, 0));
}

/// A compact script whose bundle would copy many conditions into many topics, and one whose
/// table operands would each list many keys: each is refused under the script budget, having
/// made at most the budget's items (so quickly, without the copies), and the export goes on
/// with the scripts after them.
#[test]
#[allow(clippy::disallowed_types)] // a test may read the clock
fn amplifying_scripts_are_refused_quickly_and_the_export_goes_on() {
    // 2000 conditions, then 200 keyword blocks: some 800 000 condition copies.
    let mut b = Body::default();
    b.put(&[0xf7]);
    for _ in 0..2000 {
        b.put(&[0xa1, 0x01, 0xa7]);
    }
    for _ in 0..200 {
        b.put(b"\xefx\xf6\xee");
    }
    b.put(&[0xff]);
    let conditions = b.script();
    // 500 computed-index prints of one 2500-string table: some 1.25 million keys.
    let mut b = Body::default();
    let table = BODY + 500 * 9 + 1;
    for _ in 0..500 {
        b.put(&[&[0xb5, 0xd2][..], &table.to_le_bytes(), &[0x05, 0xb2, 0xa7]].concat());
    }
    b.put(&[0xb6]);
    for _ in 0..2500 {
        b.put(b"a\0");
    }
    let keys = b.script();
    for s in [&conditions, &keys] {
        assert!(
            s.len() <= super::super::SCRIPT_LEN,
            "within the script buffer"
        );
        assert!(disassemble(b'a', 1, s).is_ok(), "the disassembler takes it");
        let t = std::time::Instant::now();
        let (bundle, spent) = Bundle::build_within(b'a', 1, s, &[], bundle::SCRIPT_BUDGET);
        let ms = t.elapsed().as_millis();
        assert_eq!(bundle.refused.as_deref(), Some(bundle::OVER_BUDGET));
        assert_eq!(spent, bundle::SCRIPT_BUDGET, "at most the budget made");
        // The clock covers the disassembly too (some 0.1 s here in the gate profile).
        assert!(ms < 5000, "refused in {ms} ms");
    }
    let (normal, _) = conversation();
    let a: Vec<Option<Vec<u8>>> = vec![
        None,
        Some(raw(&conditions)),
        Some(raw(&keys)),
        Some(raw(&normal)),
    ];
    let out = export(&files(&[("converse.a", lib(&a))])).unwrap();
    let refused: Vec<Option<&str>> = out.iter().map(|b| b.refused.as_deref()).collect();
    assert_eq!(
        refused,
        [Some(bundle::OVER_BUDGET), Some(bundle::OVER_BUDGET), None]
    );
    let member = Member {
        actor: 3,
        portrait: Some((b'a', 2)),
        traits: None,
    };
    assert_eq!(out[2], Bundle::build(b'a', 3, &normal, &[member]));
    // The export's budget holds this many scripts at the script budget: once they spent it,
    // every script after is refused, and the export still ends with every bundle.
    let full = bundle::EXPORT_BUDGET / bundle::SCRIPT_BUDGET;
    let mut a: Vec<Option<Vec<u8>>> = vec![None];
    a.extend((0..full).map(|_| Some(raw(&conditions))));
    a.push(Some(raw(&normal)));
    let out = export(&files(&[("converse.a", lib(&a))])).unwrap();
    assert_eq!(out.len(), full + 1);
    assert!(
        out.iter()
            .all(|b| b.refused.as_deref() == Some(bundle::OVER_BUDGET))
    );
}

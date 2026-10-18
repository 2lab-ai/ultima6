//! The disassembler on synthetic scripts built here (no game bytes): every op form, nested
//! expressions, keyword blocks, table strings, input ops, escapes, split runs, data-in-code
//! tables, refusals and random bytes; the op tables against the interpreter's own arms; and
//! the cross-check on a synthetic conversation.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::super::Cx;
use super::super::eval::eval;
use super::super::exec::exec;
use super::super::tests::{chars, files, lib, packed, scroll, state, talk_to};
use super::catalog::{Catalog, FORMAT, SLOT_BUFFER_CAPACITY, ScriptUnits, hash64};
use super::decode::Decoder;
use super::ops::{self, Escape, Flow, STATEMENT_OPS, Shape};
use super::trace::{self, Decoded};
use super::*;
use crate::engine::Engine;
use crate::files::Files;
use crate::state::State;

/// `ff 05 "Nm" f1 "look" f2` and `body`: the greeting and what follows start at [`BODY`].
fn script(body: &[u8]) -> Vec<u8> {
    [&b"\xff\x05Nm\xf1look\xf2"[..], body].concat()
}

const BODY: u32 = 10;

/// A `b0` to `at`.
fn jump(at: u32) -> Vec<u8> {
    [&[0xb0][..], &at.to_le_bytes()].concat()
}

/// A `d2` literal (a table address).
fn lit32(v: u32) -> Vec<u8> {
    [&[0xd2][..], &v.to_le_bytes()].concat()
}

fn ok(b: &[u8]) -> Disasm {
    let d = disassemble(b'a', 7, b).unwrap_or_else(|r| panic!("refused: {r}"));
    assert!(covers_once(&d), "{d:#?}");
    d
}

fn refused(b: &[u8]) -> Refusal {
    disassemble(b'a', 7, b).expect_err("refused")
}

fn covers_once(d: &Disasm) -> bool {
    let mut at = 0;
    for it in &d.items {
        if it.offset != at || it.len == 0 {
            return false;
        }
        at = it.end();
    }
    at == d.len
}

fn item(d: &Disasm, at: u32) -> &Item {
    d.items
        .iter()
        .find(|i| i.offset == at)
        .unwrap_or_else(|| panic!("no item at {at}: {:#?}", d.items))
}

fn op_at(d: &Disasm, at: u32) -> &Op {
    match &item(d, at).kind {
        Kind::Op(op) => op,
        k => panic!("not an op at {at}: {k:?}"),
    }
}

/// Short names of the items in order, for structure checks.
fn shape(d: &Disasm) -> Vec<String> {
    d.items
        .iter()
        .map(|i| match &i.kind {
            Kind::Header => "hdr".into(),
            Kind::Name => "name".into(),
            Kind::Marker(Marker::Look) => "f1".into(),
            Kind::Marker(Marker::Greeting) => "f2".into(),
            Kind::Text(_) => format!("text{}", i.len),
            Kind::Op(op) => op.info().name.into(),
            Kind::Keywords(k) => format!("kw{}", k.len()),
            Kind::BlockEnd => "ee".into(),
            Kind::Input(x) => trace::input_kind(x).into(),
            Kind::Stop(s) => format!("{s:?}"),
            Kind::Table(t) => format!("table{}", t.entries.len()),
            Kind::Unreached => format!("unreached{}", i.len),
        })
        .collect()
}

// ---- structure ---------------------------------------------------------------------------------

#[test]
fn header_look_greeting_and_a_keyword_block_with_several_keywords_and_a_star() {
    // f7 at BODY + 3; the blocks jump back to it.
    let ask = BODY + 3;
    let mut body = b"Hi!\xf7\xefname,job,ab\xf6I am.".to_vec();
    body.extend(jump(ask));
    body.extend(b"\xee\xef*\xf6What?");
    body.extend(jump(ask));
    // The skip loops after the last block land on the end.
    body.extend(b"\xee\xff");
    let s = script(&body);
    let d = ok(&s);
    assert_eq!(
        shape(&d),
        [
            "hdr", "name", "f1", "text4", "f2", "text3", "ask", "kw3", "text5", "jump", "ee",
            "kw1", "text5", "jump", "ee", "End"
        ]
    );
    let Kind::Keywords(k) = &item(&d, ask + 1).kind else {
        panic!()
    };
    let words: Vec<&[u8]> = k.iter().map(|sp| sp.bytes(&s)).collect();
    assert_eq!(words, [&b"name"[..], b"job", b"ab"]);
    assert_eq!(item(&d, 0).len, 2, "ff and the byte after it");
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
}

#[test]
fn the_look_text_ends_at_its_first_stop_and_the_greeting_follows_the_first_f2() {
    // The look text stops at `ee`; the bytes up to `f2` are never read.
    let s = b"\xff\x05Nm\xf1look\xeexx\xf2\xb6".to_vec();
    let d = ok(&s);
    assert_eq!(
        shape(&d),
        [
            "hdr",
            "name",
            "f1",
            "text4",
            "ee",
            "unreached2",
            "f2",
            "bye"
        ]
    );
    assert_eq!(refused(b"\xff\x05Nm").reason, Reason::NoLookMarker);
    assert_eq!(
        refused(b"\xff\x05Nm\xf1look").reason,
        Reason::NoGreetingMarker
    );
    assert_eq!(refused(b"\xfe\x05Nm\xf1\xf2").reason, Reason::NoHeader);
    assert_eq!(refused(b"\xff").reason, Reason::NoHeader);
}

#[test]
fn every_statement_op_form_decodes_with_its_operands() {
    let mut body = Vec::new();
    let put = |b: &[u8], body: &mut Vec<u8>| {
        let o = BODY + body.len() as u32;
        body.extend_from_slice(b);
        o
    };
    // Actor and expression operands; a nested expression with literals of each width.
    let move_object = put(
        &[
            0xc9, 0xd4, 0x34, 0x12, 0xa7, 3, 4, 0x90, 0xd3, 2, 0x92, 0xa7, 0xeb, 0xa7, 1, 0xa7,
        ],
        &mut body,
    );
    let print_slot = put(&[0xb5, 0x00, 0x05, 0xb3, 0xa7], &mut body);
    let print_nothing = put(&[0xb5, 0xd5], &mut body);
    let assign_int = put(&[0xa6, 0x04, 0xb2, 0xa8, 7, 0xa7], &mut body);
    let assign_slot = put(&[0xa6, 0x02, 0xb3, 0xa8, 0x03, 0xb3], &mut body);
    let nop = put(&[0xa7], &mut body);
    let tables = BODY + body.len() as u32 + 60;
    let print_table = put(&[&[0xb5][..], &lit32(tables), &[0xb1]].concat(), &mut body);
    let assign_word = put(
        &[&[0xa6][..], &lit32(tables + 6), &[1, 0xa8, 9, 0xa7]].concat(),
        &mut body,
    );
    let assign_str = put(
        &[&[0xa6, 0x01, 0xb3, 0xa8][..], &lit32(tables), &[0, 0xa7]].concat(),
        &mut body,
    );
    let bye = put(&[0xb6], &mut body);
    body.resize((tables - BODY) as usize, 0);
    body.extend(b"one\0");
    body.extend([0xb8, 0, 0x10, 0, 0x20, 0, 0xb8]);
    let s = script(&body);
    let d = ok(&s);
    let o = op_at(&d, move_object);
    let Operands::Args(args) = &o.operands else {
        panic!()
    };
    assert_eq!(
        args.iter()
            .map(|a| (a.actor, a.expr.len))
            .collect::<Vec<_>>(),
        [(false, 4), (false, 7), (true, 2), (true, 2)]
    );
    assert_eq!(
        args[1]
            .expr
            .tokens
            .iter()
            .map(|t| t.kind)
            .collect::<Vec<_>>(),
        [
            TokenKind::Value(3),
            TokenKind::Value(4),
            TokenKind::Op(0x90),
            TokenKind::Lit8(2),
            TokenKind::Op(0x92)
        ]
    );
    assert_eq!(args[0].expr.constant(), Some(0x1234));
    assert_eq!(item(&d, move_object).len, 16);
    assert_eq!(
        op_at(&d, print_slot).operands,
        Operands::Print(Print::Slot {
            kind: 0,
            slot: 5,
            skipped: 0xa7
        })
    );
    assert_eq!(
        op_at(&d, print_nothing).operands,
        Operands::Print(Print::Nothing)
    );
    assert!(matches!(
        &op_at(&d, assign_int).operands,
        Operands::Assign(Assign::Int { var: 4, value }) if value.constant() == Some(7)
    ));
    assert_eq!(
        op_at(&d, assign_slot).operands,
        Operands::Assign(Assign::StrFromSlot {
            var: 2,
            kind: 0xb3,
            src: 3
        })
    );
    assert_eq!(
        item(&d, assign_slot).len,
        6,
        "the a7 after it is its own op"
    );
    assert_eq!(op_at(&d, nop).code, 0xa7);
    assert!(matches!(
        &op_at(&d, print_table).operands,
        Operands::Print(Print::Table(TableRef { addr, index: None })) if *addr == tables
    ));
    assert_eq!(item(&d, print_table).len, 7);
    // The word element's index ends in the a8 that separates the value.
    assert!(matches!(
        &op_at(&d, assign_word).operands,
        Operands::Assign(Assign::TableWord { table, value })
            if table.addr == tables + 6 && table.index.as_ref().is_some_and(|e| e.end == 0xa8)
                && value.constant() == Some(9)
    ));
    assert_eq!(item(&d, assign_word).len, 10);
    assert!(matches!(
        &op_at(&d, assign_str).operands,
        Operands::Assign(Assign::StrFromTable { var: 1, kind: 0xb3, table })
            if table.index.as_ref().and_then(Expr::constant) == Some(0)
    ));
    assert_eq!(op_at(&d, bye).code, 0xb6);
    // The tables: strings up to the b8, words up to theirs.
    let Kind::Table(t) = &item(&d, tables).kind else {
        panic!()
    };
    assert!(!t.words && t.terminated && t.entries.len() == 1);
    assert_eq!(item(&d, tables + 5).kind, Kind::Unreached);
    let Kind::Table(w) = &item(&d, tables + 6).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(w.words && w.terminated && w.entries.len() == 2);
}

#[test]
fn a_jump_follows_its_target_and_the_bytes_after_it_are_unreached() {
    let mut body = jump(BODY + 8);
    body.extend([0x80, 0x80, 0x80]);
    body.extend(b"Hi\xb6");
    let d = ok(&script(&body));
    assert_eq!(
        op_at(&d, BODY).operands,
        Operands::Jump { target: BODY + 8 }
    );
    assert_eq!(&shape(&d)[5..], ["jump", "unreached3", "text2", "bye"]);
    assert_eq!(refused(&script(&jump(0x7000))).reason, Reason::JumpOutside);
}

#[test]
fn if_and_else_follow_both_ways() {
    // a1 cond; then: "A" a3; else: "B" a2; then the end.
    let body = b"\xa1\x01\xa7A\xa3B\xa2\xb6";
    let d = ok(&script(body));
    assert_eq!(
        &shape(&d)[5..],
        ["if", "text1", "else", "text1", "endif", "bye"]
    );
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
}

#[test]
fn input_ops_decode_and_a_wrong_tag_is_refused() {
    let body = b"\xf8yn\xefy\xf6\xb6\xee\xf9\x03\xb3\xfa\xfb\x04\xb2\xfc\x05\xb2\xff";
    let d = ok(&script(body));
    assert_eq!(
        &shape(&d)[5..],
        [
            "keys", "kw1", "bye", "ee", "line", "key", "number", "digit", "End"
        ]
    );
    assert_eq!(
        item(&d, BODY).kind,
        Kind::Input(Input::Keys(Span {
            offset: BODY + 1,
            len: 2
        }))
    );
    assert_eq!(
        refused(&script(b"\xf9\x03\xb2")).reason,
        Reason::Malformed(0xf9)
    );
    assert_eq!(
        refused(&script(b"\xb5\x00\x05\xb2\xa7")).reason,
        Reason::Malformed(0xb5)
    );
    assert_eq!(
        refused(&script(b"\xa6\x01\x00\xa8\x05\xa7")).reason,
        Reason::Malformed(0xa6),
        "kind 0 would write the script's first word"
    );
}

#[test]
fn escapes_inside_a_run_are_its_placeholders() {
    let run = b"Hi $N, #3 coins/s\\s *@word &x& <r> `b ^c 100% end$";
    let mut body = run.to_vec();
    body.push(0xb6);
    let s = script(&body);
    let d = ok(&s);
    let Kind::Text(r) = &item(&d, BODY).kind else {
        panic!()
    };
    let esc: Vec<Escape> = r.placeholders().filter_map(|p| p.escape).collect();
    assert_eq!(
        esc,
        [
            Escape::Str(Some(23)),
            Escape::Int(Some(3)),
            Escape::Singular,
            Escape::Plural,
            Escape::Wait,
            Escape::Keyword,
            Escape::Hidden,
            Escape::Hidden,
            Escape::RunicOn,
            Escape::RunicOff,
            Escape::Capital,
            Escape::Glyph(Some(b'c')),
            Escape::Literal,
            Escape::Str(None),
        ]
    );
    // The parts cover the run exactly once.
    let mut at = BODY;
    for p in &r.parts {
        assert_eq!(p.offset, at);
        at += p.len;
    }
    assert_eq!(at, BODY + run.len() as u32);
    let units = ScriptUnits::new(&d, &s);
    let ph: Vec<&str> = units.runs[1]
        .placeholders
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(ph[..3], ["$N", "#3", "/"]);
}

#[test]
fn the_escape_set_is_the_escape_layers() {
    // An ordinary character put through the escape layer prints itself; an escape does not.
    for b in 0x21..0x7fu8 {
        let mut e = Engine::with_game(state(&[]), Files::default(), move |st, io| async move {
            crate::ui::printf::text_out(&st, &io, i16::from(b)).await;
        });
        for k in chars("  ") {
            e.tick(&[k], 16);
        }
        let printed = scroll(&e.state());
        let ordinary = printed == char::from(b).to_string();
        assert_eq!(
            ordinary,
            Escape::start(b).is_none(),
            "byte {b:#04x} printed {printed:?}"
        );
    }
}

#[test]
fn a_run_split_across_an_op_is_two_runs() {
    let d = ok(&script(b"Hello \xa7world.\xb6"));
    assert_eq!(&shape(&d)[5..], ["text6", "nop", "text6", "bye"]);
}

#[test]
fn a_jump_into_text_splits_the_run_there() {
    // "Hello\n" then f7; the block jumps to the newline before f7.
    let nl = BODY + 5;
    let mut body = b"Hello\n\xf7\xef*\xf6Ok".to_vec();
    body.extend(jump(nl));
    body.push(0xee);
    let d = ok(&script(&body));
    assert_eq!(
        &shape(&d)[5..],
        ["text5", "text1", "ask", "kw1", "text2", "jump", "ee"]
    );
    assert_eq!(d.run_splits, [nl]);
}

#[test]
fn data_in_code_tables_are_data_never_ops() {
    // b7, c3 and b4 name tables placed after a jump; their bytes hold op bytes the executor
    // has no arm for (0x80), which would refuse the script if they were decoded as code.
    let strings = BODY + 44;
    let words = strings + 12;
    let more = words + 7;
    let mut body = Vec::new();
    body.extend([0xa6, 0x01, 0xb2, 0xa8]);
    body.extend(lit32(strings));
    body.extend([0x00, 0xb7, 0xa7]);
    body.extend([0xa6, 0x02, 0xb2, 0xa8]);
    body.extend(lit32(words));
    body.extend([0x04, 0xc3, 0xa7]);
    body.extend([0xa6, 0x03, 0xb2, 0xa8]);
    body.extend(lit32(more));
    body.extend([0x01, 0xb4, 0xa7]);
    body.extend(jump(more + 4));
    body.resize((strings - BODY) as usize, 0);
    body.extend(b"\x80ab\0cd\x80\0\xb8");
    body.resize((words - BODY) as usize, 0);
    body.extend([0x80, 0x80, 0x10, 0x00, 0x80, 0x00, 0xb8]);
    body.extend([0x80, 0x81, 0x80, 0x82]);
    body.extend(b"Done\xb6");
    let s = script(&body);
    let d = ok(&s);
    let table = |at: u32| match &item(&d, at).kind {
        Kind::Table(t) => t.clone(),
        k => panic!("{k:?}"),
    };
    let t = table(strings);
    assert!(!t.words && t.terminated);
    assert_eq!(
        t.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [&b"\x80ab"[..], b"cd\x80"]
    );
    let w = table(words);
    assert!(w.words && w.terminated && w.entries.len() == 3);
    let m = table(more);
    assert!(m.words && !m.terminated && m.entries.len() == 2);
    assert!(
        matches!(item(&d, more + 4).kind, Kind::Text(_)),
        "the jump target is code"
    );
    // The same address as strings and as words is refused, as is a computed address.
    let mut body = vec![0xa6, 0x01, 0xb2, 0xa8];
    body.extend(lit32(BODY + 30));
    body.extend([0x00, 0xb7, 0xa7, 0xa6, 0x01, 0xb2, 0xa8]);
    body.extend(lit32(BODY + 30));
    body.extend([0x00, 0xc3, 0xa7, 0xb6]);
    body.resize(40, 0);
    assert_eq!(refused(&script(&body)).reason, Reason::TableKinds);
    let body = [0xa6, 0x01, 0xb2, 0xa8, 0x05, 0xb2, 0x00, 0xb7, 0xa7, 0xb6];
    assert_eq!(refused(&script(&body)).reason, Reason::TableNotLiteral);
    // A table op with nothing below its operand reads below the stack: no table, a hazard.
    let d = ok(&script(&[
        0xa6, 0x01, 0xb2, 0xa8, 0x05, 0xb2, 0xb4, 0xa7, 0xb6,
    ]));
    assert!(
        d.hazards.iter().any(|h| h.kind == HazardKind::Underflow),
        "{:?}",
        d.hazards
    );
    assert_eq!(
        refused(&script(
            &[&[0xb5][..], &lit32(0x7000), &[0xb1, 0xb6]].concat()
        ))
        .reason,
        Reason::TableOutside
    );
}

#[test]
fn table_strings_carry_the_slots_they_are_copied_into() {
    let t = BODY + 40;
    let mut body = Vec::new();
    // a6 3 b3 a8 d2 t 1 a7: string 1 into slot 3 (a buffer); a6 17 b3 a8 d2 t b1: string 0
    // into slot 0x17 (where the game points it); b5 d2 t 0 a7: print string 0.
    body.extend([0xa6, 0x03, 0xb3, 0xa8]);
    body.extend(lit32(t));
    body.extend([0x01, 0xa7]);
    body.extend([0xa6, 0x17, 0xb3, 0xa8]);
    body.extend(lit32(t));
    body.push(0xb1);
    body.push(0xb5);
    body.extend(lit32(t));
    body.extend([0x00, 0xa7, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"first\0second\0third\0");
    let s = script(&body);
    let d = ok(&s);
    let units = ScriptUnits::new(&d, &s);
    let strings: Vec<(String, u32, Vec<u8>, Option<u32>)> = units
        .strings
        .iter()
        .map(|u| (u.unit.text.clone(), u.index, u.slots.clone(), u.capacity))
        .collect();
    assert_eq!(
        strings,
        [
            ("first".into(), 0, vec![0x17], None),
            ("second".into(), 1, vec![3], Some(SLOT_BUFFER_CAPACITY)),
            ("third".into(), 2, vec![], None),
        ]
    );
    assert_eq!(units.strings[1].unit.offset, t + 6);
    assert_eq!(units.strings[1].unit.hash, hash64(b"second"));
    // A constant index past the table's strings is refused.
    let mut body = vec![0xb5];
    body.extend(lit32(BODY + 20));
    body.extend([0x05, 0xa7, 0xb6]);
    body.resize(20, 0);
    body.extend(b"a\0b\0");
    assert_eq!(refused(&script(&body)).reason, Reason::TableIndex);
}

#[test]
fn an_unknown_opcode_is_refused_with_a_named_reason() {
    for code in (0x80..0xee).filter(|&b| ops::statement_op(b).is_none() && b != 0xef) {
        let r = refused(&script(&[code, 0xb6]));
        assert_eq!(
            r,
            Refusal {
                at: BODY,
                reason: Reason::UnknownOp(code)
            }
        );
        assert_eq!(
            r.to_string(),
            format!("unknown op {code:#04x} at {BODY:#06x}")
        );
    }
}

#[test]
fn skip_loops_that_stop_inside_an_operand_are_hazards() {
    // An answered block skips to the first ee: here the low byte of a d4 literal.
    let mut body = b"\xf7\xef*\xf6".to_vec();
    body.extend([0xc4, 0xd4, 0xee, 0x00, 0xa7]);
    body.extend(jump(BODY));
    body.push(0xee);
    let d = ok(&script(&body));
    assert!(
        d.hazards
            .iter()
            .any(|h| h.kind == HazardKind::Scan(Scan::Answered)),
        "{:?}",
        d.hazards
    );
    // A false a1 skips to the first a2 / a3, not stepping over d2 literals.
    let mut body = vec![0xa1, 0x00, 0xa7, 0xc4];
    body.extend(lit32(0xa2));
    body.extend([0xa7, 0xa2, 0xb6]);
    let d = ok(&script(&body));
    assert!(
        d.hazards
            .iter()
            .any(|h| h.kind == HazardKind::Scan(Scan::If)),
        "{:?}",
        d.hazards
    );
}

#[test]
fn a_landing_whose_path_would_refuse_the_script_is_dropped() {
    // The block's answer jumps back to the question, so only its skip loops get past its `ee`,
    // onto a byte the executor has no arm for: those paths are dropped, the rest decodes.
    let mut body = b"\xf7\xef*\xf6Ok".to_vec();
    body.extend(jump(BODY));
    body.extend([0xee, 0x80, 0xff]);
    let d = ok(&script(&body));
    assert_eq!(
        &shape(&d)[5..],
        ["ask", "kw1", "text2", "jump", "unreached3"]
    );
    for scan in [Scan::NoMatch, Scan::Answered] {
        assert!(
            d.hazards.contains(&Hazard {
                at: BODY + 12,
                kind: HazardKind::Dropped(scan, Reason::UnknownOp(0x80)),
            }),
            "{:?}",
            d.hazards
        );
    }
}

#[test]
fn truncated_scripts_are_refused_and_never_panic() {
    let t = BODY + 60;
    let mut body = b"Hi $P.\xf7\xefname,job\xf6I am.".to_vec();
    body.extend([0xa6, 0x01, 0xb3, 0xa8]);
    body.extend(lit32(t));
    body.extend([
        0x00, 0xa7, 0xc9, 0x01, 0xa7, 0x02, 0xa7, 0xeb, 0xa7, 0x01, 0xa7,
    ]);
    body.extend(jump(BODY + 6));
    body.extend(b"\xee\xf9\x00\xb3\xb6");
    body.resize((t - BODY) as usize, 0);
    body.extend(b"str\0");
    let s = script(&body);
    ok(&s);
    let mut refusals = 0;
    for n in 0..s.len() {
        match disassemble(b'a', 7, &s[..n]) {
            Ok(d) => assert!(covers_once(&d)),
            Err(_) => refusals += 1,
        }
    }
    assert!(refusals > s.len() / 2, "{refusals} of {}", s.len());
    // Cut inside the c9's operands.
    let cut = BODY as usize + 25;
    assert!(matches!(
        disassemble(b'a', 7, &s[..cut]),
        Err(Refusal {
            reason: Reason::Truncated | Reason::PastEnd,
            ..
        })
    ));
}

#[test]
fn random_bytes_never_panic_and_every_decode_covers_its_script_once() {
    // xorshift; bytes biased towards the structure bytes so that decodes get far.
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let common = [
        0xa7, 0xa8, 0xb0, 0xa1, 0xa2, 0xa3, 0xb5, 0xa6, 0xd2, 0xd3, 0xd4, 0xb2, 0xb3, 0xef, 0xf6,
        0xee, 0xf7, 0xf2, 0xf1, 0xb6, b',', b'a', 0x00, 0xb8, 0xb1,
    ];
    let (mut decoded, mut runs) = (0, 0);
    for _ in 0..20_000 {
        let len = 2 + (next() % 200) as usize;
        let mut b: Vec<u8> = (0..len)
            .map(|_| {
                let r = next();
                if r % 3 == 0 {
                    common[(r >> 8) as usize % common.len()]
                } else if r % 3 == 1 {
                    b'a' + (r >> 8) as u8 % 26
                } else {
                    (r >> 8) as u8
                }
            })
            .collect();
        b[0] = 0xff;
        if let Ok(d) = disassemble(b'b', 1, &b) {
            assert!(covers_once(&d), "{b:02x?}");
            decoded += 1;
            runs += d
                .items
                .iter()
                .filter(|i| matches!(i.kind, Kind::Text(_)))
                .count();
            let _ = ScriptUnits::new(&d, &b);
        }
    }
    assert!(
        decoded > 100 && runs > 100,
        "{decoded} decoded, {runs} runs"
    );
}

// ---- the op tables against the interpreter ---------------------------------------------------

/// Runs the executor on op `script[0]` (already read) with `keys`; returns its result and the
/// read position after it.
fn exec_on(script: &[u8], keys: &str) -> (i16, State) {
    let mut s = state(script);
    s.converse.pc = 1;
    s.actors.calendar.hour = 4;
    let out = Rc::new(RefCell::new(None));
    let o = Rc::clone(&out);
    let op = script[0];
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        let r = exec(&Cx::new(&st, &io), op, 9).await;
        *o.borrow_mut() = Some(r);
    });
    for k in chars(keys) {
        e.tick(&[k], 16);
    }
    for _ in 0..2000 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished(), "op {op:#04x} ended");
    let r = out.borrow_mut().take().expect("result");
    (r, e.state().clone())
}

#[test]
fn the_statement_op_table_is_the_executors_arms() {
    assert_eq!(STATEMENT_OPS.len(), 28);
    for code in 0x80..0xee_u8 {
        if code == ops::KEYWORDS {
            continue;
        }
        let Some(info) = ops::statement_op(code) else {
            let (r, s) = exec_on(&[code, 0xa7, 0xa7, 0xa7, 0xa7], "");
            assert_eq!(r, -1, "{code:#04x} takes the default arm");
            assert_eq!(s.converse.pc, 1, "{code:#04x} reads nothing");
            assert!(scroll(&s).contains("[UnknownCommand]"));
            continue;
        };
        // Operands the decoder reads for this op, then bytes it must not read.
        let mut b = vec![code];
        match info.shape {
            Shape::Args(args) => {
                for _ in args.iter() {
                    b.extend([0x01, 0xa7]);
                }
            }
            Shape::Jump => b.extend([0x08, 0, 0, 0]),
            Shape::Print => b.push(0xd5),
            Shape::Assign => b.extend([0x05, 0xb2, 0xa8, 0x07, 0xa7]),
        }
        b.extend([0xa2, 0xa2, 0x00, 0x00]);
        let (len, decoded) = Decoder::new(&b, BTreeSet::new())
            .op(0, code)
            .unwrap_or_else(|r| panic!("{code:#04x}: {r}"));
        assert_eq!(decoded.code, code);
        let (r, s) = exec_on(&b, "    ");
        assert_ne!(r, -1, "{code:#04x} has an arm");
        let want = match info.flow {
            Flow::Jump => 8,
            // a3 skips past the next a2.
            Flow::Else => len + 1,
            _ => len,
        };
        assert_eq!(
            u32::from(s.converse.pc),
            want,
            "{code:#04x} reads {len} bytes"
        );
        assert_eq!(s.converse.bye, info.flow == Flow::Bye, "{code:#04x}");
    }
    // The other forms of b5 and a6 against the executor.
    for b in [
        vec![0xb5, 0x00, 0x05, 0xb3, 0xa7],
        [&[0xb5][..], &lit32(20), &[0xb1]].concat(),
        [&[0xb5][..], &lit32(20), &[0x00, 0xa7]].concat(),
        [&[0xa6][..], &lit32(20), &[0x01, 0xa8, 0x07, 0xa7]].concat(),
        [&[0xa6][..], &lit32(20), &[0xb1, 0xa8, 0x07, 0xa7]].concat(),
        [&[0xa6, 0x02, 0xb3, 0xa8][..], &lit32(20), &[0x00, 0xa7]].concat(),
        [&[0xa6, 0x02, 0xb3, 0xa8][..], &lit32(20), &[0xb1]].concat(),
        vec![0xa6, 0x02, 0xb3, 0xa8, 0x03, 0xb3],
    ] {
        let mut s = b.clone();
        s.resize(20, 0xa2);
        s.extend(b"ab\0cd\0");
        let (len, _) = Decoder::new(&s, BTreeSet::new()).op(0, s[0]).unwrap();
        let (r, after) = exec_on(&s, "");
        assert_eq!(r, 0, "{b:02x?}");
        assert_eq!(u32::from(after.converse.pc), len, "{b:02x?}");
    }
}

#[test]
fn expression_lengths_are_the_evaluators() {
    // Tokens of every lexical class; ops without waits or script writes.
    // Every op token is one byte whatever it does, so pure ops (and two bytes that are no op)
    // cover the grammar.
    let ops_ = [
        0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x90, 0x91, 0x92, 0x94, 0x95, 0xa0, 0xb2, 0xb3, 0xbc,
        0xbd, 0xeb, 0x99,
    ];
    let mut x: u32 = 0x1234_5678;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    for _ in 0..300 {
        let mut b = Vec::new();
        for _ in 0..next() % 8 {
            match next() % 5 {
                0 => b.extend([0xd2, next() as u8, 0xa7, 0xa8, next() as u8]),
                1 => b.extend([0xd3, 0xa7]),
                2 => b.extend([0xd4, 0xa8, next() as u8]),
                3 => b.push((next() % 0x80) as u8),
                _ => b.push(ops_[next() as usize % ops_.len()]),
            }
        }
        b.push(if next() % 2 == 0 { 0xa7 } else { 0xa8 });
        let len = b.len();
        b.extend([0x55; 8]);
        let e = Decoder::new(&b, BTreeSet::new()).expr(0).unwrap();
        assert_eq!(e.len as usize, len);
        let mut st = state(&b);
        st.converse.pc = 0;
        let out = Rc::new(RefCell::new(None));
        let o = Rc::clone(&out);
        let mut eng = Engine::with_game(st, Files::default(), move |st, io| async move {
            let v = eval(&Cx::new(&st, &io)).await;
            *o.borrow_mut() = Some(v);
        });
        for _ in 0..50 {
            if eng.finished() {
                break;
            }
            eng.tick(&[], 16);
        }
        assert!(eng.finished());
        assert_eq!(usize::from(eng.state().converse.pc), len, "{b:02x?}");
    }
    assert_eq!(ops::EXPR_OPS.len(), 42);
}

// ---- the cross-check on a synthetic conversation ----------------------------------------------

/// A conversation for actor 3: greeting with `$P`, keyword blocks, a table string printed and
/// one copied into slot 0 and printed by `$0`, a `*` wait, a jump back into the text before
/// the question, bye.
fn conversation() -> Vec<u8> {
    let mut s = b"\xff\x03Ann\xf1a tester\xf2Hello $P.".to_vec();
    let nl = s.len() as u32;
    s.extend(b"\n\xf7\xefname\xf6I am Ann.");
    let table = 200u32;
    s.extend(jump(nl));
    s.extend(b"\xee\xefjob\xf6I test");
    s.extend([0xa6, 0x00, 0xb3, 0xa8]);
    s.extend(lit32(table));
    s.extend([0x01, 0xa7]);
    s.extend(b" and $0*. ");
    s.push(0xb5);
    s.extend(lit32(table));
    s.push(0xb1);
    s.push(b'!');
    s.extend(jump(nl));
    s.extend(b"\xee\xefbye\xf6Bye.\xb6\xee\xef*\xf6What?");
    s.extend(jump(nl));
    s.push(0xee);
    s.resize(table as usize, 0);
    s.extend(b"alpha\0beta\0");
    s
}

fn conversation_files(script: &[u8]) -> Files {
    let mut a: Vec<Option<Vec<u8>>> = vec![None; 0x63];
    a[3] = Some(packed(script));
    files(&[("talkfile.a", lib(&a))])
}

#[test]
fn the_interpreter_reads_only_what_the_disassembler_found() {
    let s = conversation();
    let d = ok(&s);
    assert_eq!(d.run_splits.len(), 1);
    let scripts: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 3),
        Decoded {
            bytes: &s,
            disasm: &d,
        },
    )]
    .into();
    trace::start(true);
    let end = talk_to(
        state(&[]),
        conversation_files(&s),
        3,
        "name\njob\n \nxyz\nbye\n",
    );
    let events = trace::stop();
    let text = scroll(&end).replace('\n', " ");
    assert!(
        text.contains("Hello Hero.") && text.contains("beta") && text.contains("alpha!"),
        "{text}"
    );
    let n = trace::check(&events, &scripts);
    assert_eq!(n.failures(), 0, "{n:?}");
    assert!(
        n.offsets > 100 && n.filled >= 6 && n.table_strings == 2,
        "{n:?}"
    );
    // The check is not vacuous: a wrong disassembly or a wrong byte is found.
    let mut wrong = d.clone();
    for it in &mut wrong.items {
        if matches!(it.kind, Kind::Text(_)) && it.offset > BODY {
            it.kind = Kind::Unreached;
            break;
        }
    }
    let bad: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 3),
        Decoded {
            bytes: &s,
            disasm: &wrong,
        },
    )]
    .into();
    assert!(trace::check(&events, &bad).mismatches > 0);
    let mut other = s.clone();
    let hello = other.windows(5).position(|w| w == b"Hello").unwrap();
    other[hello] = b'J';
    let bytes: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 3),
        Decoded {
            bytes: &other,
            disasm: &d,
        },
    )]
    .into();
    let n = trace::check(&events, &bytes);
    assert!(n.byte_mismatches == 1 && n.fill_mismatches == 1, "{n:?}");
}

// ---- the catalog -------------------------------------------------------------------------------

#[test]
fn the_catalog_hash_is_fnv1a_64_and_its_json_reads_back() {
    // The vectors u6-hdpack pins for its content hash.
    assert_eq!(hash64(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(hash64(b"a"), 0xaf63_dc4c_8601_ec8c);
    let s = conversation();
    let bad = script(&[0x80]);
    let c = Catalog::build([(b'a', 3, s.as_slice()), (b'b', 9, bad.as_slice())]);
    assert_eq!(c.format, FORMAT);
    let units = &c.scripts[0];
    assert_eq!((units.archive, units.item), ('a', 3));
    assert_eq!(units.hash, hash64(&s));
    assert_eq!(units.name.as_ref().map(|n| n.text.as_str()), Some("Ann"));
    assert!(
        units
            .runs
            .iter()
            .any(|r| r.unit.text == "Hello $P." && r.placeholders == ["$P"])
    );
    assert_eq!(
        units
            .keywords
            .iter()
            .map(|k| k.text.as_str())
            .collect::<Vec<_>>(),
        ["name", "job", "bye", "*"]
    );
    assert_eq!(
        units
            .strings
            .iter()
            .map(|k| (k.unit.text.as_str(), k.slots.clone()))
            .collect::<Vec<_>>(),
        [("alpha", vec![]), ("beta", vec![0])]
    );
    for r in &units.runs {
        let o = r.unit.offset as usize;
        assert_eq!(r.unit.hash, hash64(&s[o..o + r.unit.len as usize]));
    }
    assert_eq!(
        c.scripts[1].refused.as_deref(),
        Some("unknown op 0x80 at 0x000a")
    );
    assert!(c.scripts[1].runs.is_empty());
    let json = c.to_json();
    assert!(json.contains(&format!("\"hash\":\"{:016x}\"", hash64(&s))));
    assert_eq!(Catalog::from_json(&json).unwrap(), c);
    let other = json.replace(FORMAT, "u6-converse-catalog/2");
    assert!(Catalog::from_json(&other).is_err());
    // Bytes from 0x80 read back unchanged.
    let high = script(&[0xb5, 0xd2, 20, 0, 0, 0, 0xb1, 0xb6, 0, 0, b'\xe9', 0xff, 0]);
    let c = Catalog::build([(b'a', 1, high.as_slice())]);
    let back = Catalog::from_json(&c.to_json()).unwrap();
    let t: Vec<u8> = back.scripts[0].strings[0]
        .unit
        .text
        .chars()
        .map(|c| c as u32 as u8)
        .collect();
    assert_eq!(t, [0xe9, 0xff]);
}

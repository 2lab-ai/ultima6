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
use super::decode::{Decoder, ORDER, Plan, ref_entry};
use super::ops::{self, Escape, Flow, STATEMENT_OPS, Shape};
use super::trace::{self, Decoded, EscState, Event, Read, Snap};
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
    assert!(covers_once(&d, b), "{d:#?}");
    d
}

fn refused(b: &[u8]) -> Refusal {
    disassemble(b'a', 7, b).expect_err("refused")
}

/// The items cover the input, every byte once, in order.
fn covers_once(d: &Disasm, input: &[u8]) -> bool {
    assert_eq!(
        d.len as usize,
        input.len(),
        "the decode's length is the input's"
    );
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
    let units = ScriptUnits::units(&d, &s);
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
    let units = ScriptUnits::units(&d, &s);
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
fn every_prefix_decodes_its_whole_length_or_is_refused_and_operand_cuts_are_truncated() {
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
    for n in 0..s.len() {
        if let Ok(d) = disassemble(b'a', 7, &s[..n]) {
            assert!(covers_once(&d, &s[..n]), "prefix {n}");
        }
    }
    // Cuts inside each operand width, on the direct path: refused where the bytes end.
    let assign = script(
        &[
            &[0xa6, 0x01, 0xb3, 0xa8][..],
            &lit32(BODY + 12),
            &[0x00, 0xa7, 0xb6, b's', 0],
        ]
        .concat(),
    );
    let call = script(&[
        0xc9, 0xd3, 0x01, 0xa7, 0xd4, 0x02, 0x00, 0xa7, 0xeb, 0xa7, 0x01, 0xa7, 0xb6,
    ]);
    let jumps = script(&[&jump(BODY + 5)[..], &[0xb6]].concat());
    for (s, cut, what) in [
        (&assign, BODY + 6, "a6's table address (d2, 32 bits)"),
        (&assign, BODY + 10, "a6's index expression, before its a7"),
        (&call, BODY + 2, "c9's first operand, a d3 literal (8 bits)"),
        (
            &call,
            BODY + 6,
            "c9's second operand, a d4 literal (16 bits)",
        ),
        (&call, BODY + 9, "c9's third operand, before its a7"),
        (&jumps, BODY + 3, "b0's address (32 bits)"),
    ] {
        ok(s);
        assert_eq!(
            disassemble(b'a', 7, &s[..cut as usize]),
            Err(Refusal {
                at: cut,
                reason: Reason::Truncated
            }),
            "cut inside {what}"
        );
    }
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
                    common[((r >> 8) % common.len() as u64) as usize]
                } else if r % 3 == 1 {
                    b'a' + (r >> 8) as u8 % 26
                } else {
                    (r >> 8) as u8
                }
            })
            .collect();
        b[0] = 0xff;
        if let Ok(d) = disassemble(b'b', 1, &b) {
            assert!(covers_once(&d, &b), "{b:02x?}");
            decoded += 1;
            runs += d
                .items
                .iter()
                .filter(|i| matches!(i.kind, Kind::Text(_)))
                .count();
            let _ = ScriptUnits::units(&d, &b);
        }
    }
    assert!(
        decoded > 100 && runs > 100,
        "{decoded} decoded, {runs} runs"
    );
}

// ---- tables and the landing trial ------------------------------------------------------------

/// `f7`, a `*` block whose answer `Ok` jumps back to the question, its `ee` at `BODY + 11`, then
/// `tail` at `BODY + 12`: only the block's skip loops (no match, answered) land on the tail.
fn after_a_block(tail: &[u8]) -> Vec<u8> {
    let mut body = b"\xf7\xef*\xf6Ok".to_vec();
    body.extend(jump(BODY));
    body.push(0xee);
    body.extend_from_slice(tail);
    script(&body)
}

/// Both skip loops of [`after_a_block`]'s block dropped for `reason`.
fn both_dropped(d: &Disasm, reason: Reason) -> bool {
    [Scan::NoMatch, Scan::Answered].iter().all(|&scan| {
        d.hazards.contains(&Hazard {
            at: BODY + 12,
            kind: HazardKind::Dropped(scan, reason),
        })
    })
}

#[test]
fn a_table_address_past_the_script_is_refused_before_any_table_is_scanned() {
    // A table at the script's tail, then (in address order) one past the end: the review's
    // repro of a panic in the table claim.
    let t = BODY + 20;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.push(0xb1);
    body.push(0xb5);
    body.extend(lit32(0x7000));
    body.extend([0xb1, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"x\0");
    assert_eq!(
        refused(&script(&body)),
        Refusal {
            at: BODY + 7,
            reason: Reason::TableOutside
        }
    );
}

#[test]
fn a_landing_path_whose_table_check_fails_is_dropped_never_the_script() {
    // An address past the end; a computed address; the direct answer's text; a byte inside the
    // direct jump's operand. Each drops the landings' path; the direct paths stay as they are.
    let cases: [(Vec<u8>, Reason); 4] = [
        (
            [&[0xb5][..], &lit32(0x7000), &[0xb1, 0xff]].concat(),
            Reason::TableOutside,
        ),
        (
            vec![0xa6, 0x01, 0xb2, 0xa8, 0x05, 0xb2, 0x00, 0xb7, 0xa7, 0xff],
            Reason::TableNotLiteral,
        ),
        (
            [&[0xb5][..], &lit32(BODY + 4), &[0xb1, 0xff]].concat(),
            Reason::TableInCode,
        ),
        (
            [&[0xb5][..], &lit32(BODY + 7), &[0xb1, 0xff]].concat(),
            Reason::TableInCode,
        ),
    ];
    for (tail, reason) in cases {
        let s = after_a_block(&tail);
        let d = ok(&s);
        assert!(both_dropped(&d, reason), "{reason:?}: {:?}", d.hazards);
        assert_eq!(
            &shape(&d)[5..],
            [
                "ask",
                "kw1",
                "text2",
                "jump",
                &format!("unreached{}", tail.len() + 1)
            ],
            "{reason:?}: the answer's text and jump are code, the rest unreached"
        );
        assert!(!d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))));
    }
    // The same uses on a direct path refuse the script.
    assert_eq!(
        refused(&script(
            &[&[0xb5][..], &lit32(0x7000), &[0xb1, 0xb6]].concat()
        ))
        .reason,
        Reason::TableOutside
    );
    let own_text = [&b"Hi\xb5"[..], &lit32(BODY), &[0xb1, 0xb6]].concat();
    assert_eq!(
        refused(&script(&own_text)),
        Refusal {
            at: BODY + 2,
            reason: Reason::TableInCode
        }
    );
    // A direct path that reaches a table named before it.
    let into = [&[0xb5][..], &lit32(BODY + 7), &[0xb1], b"zz\0"].concat();
    assert_eq!(
        refused(&script(&into)),
        Refusal {
            at: BODY + 7,
            reason: Reason::TableInCode
        }
    );
}

#[test]
fn a_landing_path_that_names_its_own_code_as_a_table_is_dropped() {
    // "Hi" at BODY + 12, then a print of the table at BODY + 12: the path read those bytes as
    // text. Nothing stays reserved for it: no stop there, no table.
    let tail = [&b"Hi\xb5"[..], &lit32(BODY + 12), &[0xb1, 0xff]].concat();
    let d = ok(&after_a_block(&tail));
    assert!(both_dropped(&d, Reason::TableInCode), "{:?}", d.hazards);
    assert!(
        !d.hazards.iter().any(|h| h.kind == HazardKind::IntoData),
        "{:?}",
        d.hazards
    );
    assert_eq!(&shape(&d)[8..], ["jump", "unreached11"]);
}

#[test]
fn a_table_after_the_last_block_stops_its_skip_loops() {
    // Block `b`'s answer prints the table right after its `ee`; block `b`'s own skip loops land
    // there (the owner's scripts place tables so), stop, and the bytes stay a table.
    let t = BODY + 27;
    let mut body = b"\xf7\xefa\xf6A".to_vec();
    body.extend(jump(BODY));
    body.extend(b"\xee\xefb\xf6\xb5");
    body.extend(lit32(t));
    body.push(0xb1);
    body.extend(jump(BODY));
    body.push(0xee);
    body.extend(b"zz\0");
    let s = script(&body);
    let d = ok(&s);
    assert_eq!(&shape(&d)[shape(&d).len() - 3..], ["jump", "ee", "table1"]);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.entries[0].bytes(&s), b"zz");
    // One hazard: the landing reaches a table (not a skip-loop mismatch).
    assert_eq!(
        d.hazards,
        [Hazard {
            at: t,
            kind: HazardKind::IntoData
        }]
    );
}

/// `f7`, a `*` block whose answer is `a1 01 a7 b0 <f7> a2` (the `a1`'s skip loop lands at
/// `BODY + 13`), then `rest` at `BODY + 13`.
fn after_an_if(rest: &[u8]) -> Vec<u8> {
    let mut body = b"\xf7\xef*\xf6\xa1\x01\xa7".to_vec();
    body.extend(jump(BODY));
    body.push(0xa2);
    body.extend_from_slice(rest);
    script(&body)
}

#[test]
fn a_landing_into_the_middle_of_a_table_stops_there_and_the_table_keeps_its_bytes() {
    // The block prints the table after it; its skip loops find the first `ee` inside the
    // table's second string and land on the byte after it. The table holds all its strings,
    // and the landing's one hazard says it reached data.
    let strings = |op: &[u8], tail: &[u8]| {
        let mut body = b"\xf7\xef*\xf6\xb5".to_vec();
        // f7 ef * f6 b5, the address, the operand, the jump.
        let t = BODY + 5 + 5 + op.len() as u32 + 5;
        body.extend(lit32(t));
        body.extend_from_slice(op);
        body.extend(jump(BODY));
        body.extend_from_slice(tail);
        (script(&body), t)
    };
    // With its b8; without it, an index that reads the second string.
    for (op, terminated) in [(&[0xb1][..], true), (&[0x01, 0xa7][..], false)] {
        let tail: &[u8] = if terminated {
            b"ab\0c\xeed\0\xb8\xff"
        } else {
            b"ab\0c\xeed\0\xff"
        };
        let (s, t) = strings(op, tail);
        let d = ok(&s);
        let Kind::Table(tb) = &item(&d, t).kind else {
            panic!("{:?}", shape(&d))
        };
        let entries: Vec<&[u8]> = tb.entries.iter().map(|e| e.bytes(&s)).collect();
        assert_eq!(entries, [&b"ab"[..], b"c\xeed"], "terminated {terminated}");
        assert_eq!(tb.terminated, terminated);
        assert_eq!(
            d.hazards,
            [Hazard {
                at: t + 5,
                kind: HazardKind::IntoData
            }],
            "terminated {terminated}"
        );
    }
    // Direct code ends a table: after the strings a use reads, the table stops there; before
    // one, the index is past the table and the script is refused.
    let t = BODY + 17;
    let into = |op: &[u8]| {
        let mut body = vec![0xb5];
        body.extend(lit32(t));
        body.extend_from_slice(op);
        body.extend(jump(t + 3));
        body.resize((t - BODY) as usize, 0);
        body.extend(b"ab\0cd\0\xb8");
        script(&body)
    };
    let d = ok(&into(&[0xb1, 0xa7, 0xa7]));
    assert_eq!(
        &shape(&d)[shape(&d).len() - 4..],
        ["table1", "text2", "Nul", "unreached1"]
    );
    assert_eq!(
        refused(&into(&[0x01, 0xa7, 0xa7])),
        Refusal {
            at: BODY,
            reason: Reason::TableIndex
        }
    );
}

/// `f7`, then `prefix`, then a `k` block whose answer prints string 2 of `t` and string 0 of
/// `u` and says bye (no `ee`, so the block's no-match loop goes on into the tables); then the
/// tables: `t` = "a" right before `u` = "b", then an `ef` block (a no-match landing reads it as
/// code) as `u`'s second string. Returns the script and `t`, `u`.
fn a_read_into_the_next_table(prefix: &[u8]) -> (Vec<u8>, u32, u32) {
    let mut body = b"\xf7".to_vec();
    body.extend_from_slice(prefix);
    body.extend(b"\xefk\xf6");
    // Then `b5 d2 t 02 a7`, `b5 d2 u b1`, `b6`.
    let t = BODY + body.len() as u32 + 8 + 7 + 1;
    let u = t + 2;
    body.push(0xb5);
    body.extend(lit32(t));
    body.extend([0x02, 0xa7, 0xb5]);
    body.extend(lit32(u));
    body.extend([0xb1, 0xb6]);
    body.extend(b"a\0b\0\xefq\xf6Z\xb6\0");
    (script(&body), t, u)
}

#[test]
fn a_constant_index_keeps_the_entries_it_reads_in_the_tables_after_its_own() {
    // String 2 of `t` is `u`'s second string, where a no-match landing finds code: `t`'s read
    // needs it, so `u` keeps it and the landing stops there. The use on a direct path (the
    // first block), and on a landing's path (a block after a first one).
    let mut later = b"\xefj\xf6A".to_vec();
    later.extend(jump(BODY));
    later.push(0xee);
    for (prefix, what) in [(&b""[..], "direct"), (&later[..], "landing")] {
        let (s, t, u) = a_read_into_the_next_table(prefix);
        let d = ok(&s);
        let strings = |at: u32| match &item(&d, at).kind {
            Kind::Table(tb) => tb.entries.len(),
            k => panic!("{what}: {k:?} {:?}", shape(&d)),
        };
        assert_eq!((strings(t), strings(u)), (1, 2), "{what}");
        assert!(
            d.hazards.contains(&Hazard {
                at: u + 2,
                kind: HazardKind::IntoData
            }),
            "{what}: {:?}",
            d.hazards
        );
        assert!(
            !d.hazards
                .iter()
                .any(|h| matches!(h.kind, HazardKind::Dropped(..))),
            "{what}: {:?}",
            d.hazards
        );
    }
}

#[test]
fn a_table_wins_over_the_code_of_a_landing_decoded_before_it() {
    // The `a1`'s landing (decoded first: its loop sorts first) reads "zz" at BODY + 13 as text;
    // block `q`, reached later by the `*` block's no-match landing, prints the table there. The
    // decode runs again with the address closed to landings: the `a1`'s landing stops there.
    let t = BODY + 13;
    let mut rest = b"zz\0\xee\xefq\xf6\xb5".to_vec();
    rest.extend(lit32(t));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xff]);
    let s = after_an_if(&rest);
    let d = ok(&s);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.entries[0].bytes(&s), b"zz");
    // One hazard: the landing reaches a table (not a skip-loop mismatch).
    assert_eq!(
        d.hazards,
        [Hazard {
            at: t,
            kind: HazardKind::IntoData
        }]
    );
}

/// After [`after_an_if`]'s `a1` (whose landing, sorted first, jumps to `x` and runs it): a
/// jump to `x`, the `*` block's `ee`, then block `q`, which `op`s the table at `x` and jumps
/// back; then `tail` at `x`. The review's shape for a table with no entry.
fn a_table_at_an_ifs_code(op: &[u8], tail: &[u8]) -> (Vec<u8>, u32) {
    // jump (5), ee, ef q f6 (3), b5 d2 x (6), op, jump (5), ee.
    let x = BODY + 13 + 5 + 1 + 3 + 6 + op.len() as u32 + 5 + 1;
    let mut rest = jump(x);
    rest.push(0xee);
    rest.extend(b"\xefq\xf6\xb5");
    rest.extend(lit32(x));
    rest.extend_from_slice(op);
    rest.extend(jump(BODY));
    rest.push(0xee);
    rest.extend_from_slice(tail);
    (after_an_if(&rest), x)
}

#[test]
fn a_landings_table_with_no_entry_drops_its_namers_or_wins_never_refuses_the_script() {
    // No NUL after `x`: the table has no entry even once the a1's code is out of it, so the
    // print's index is past it and its namers (both of the `*` block's loops) are dropped. The
    // a1's code comes back at `x`, no table holding it: tried again, the namers' print would
    // read that code, and they are dropped for it. The a1's code stays.
    let (s, x) = a_table_at_an_ifs_code(&[0xb1], b"zz\xb6");
    let d = any_order(&s);
    assert!(matches!(item(&d, x).kind, Kind::Text(_)), "{:?}", shape(&d));
    assert!(!d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))));
    for scan in [Scan::NoMatch, Scan::Answered] {
        assert!(
            d.hazards.contains(&Hazard {
                at: BODY + 19,
                kind: HazardKind::Dropped(scan, Reason::TableInCode)
            }),
            "{:?}",
            d.hazards
        );
    }
    // One string and a computed index: cut to no entry by the a1's code, the table still holds
    // its address, wins, and the a1's landing stops there.
    let (s, x) = a_table_at_an_ifs_code(&[0x01, 0xb2, 0xa7], b"zz\0");
    let d = ok(&s);
    let Kind::Table(t) = &item(&d, x).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(
        t.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [b"zz"]
    );
    assert!(
        d.hazards.contains(&Hazard {
            at: x,
            kind: HazardKind::IntoData
        }),
        "{:?}",
        d.hazards
    );
    assert!(
        !d.hazards
            .iter()
            .any(|h| matches!(h.kind, HazardKind::Dropped(..))),
        "{:?}",
        d.hazards
    );
}

/// After [`after_an_if`]'s `a1`, whose landing (sorted first) jumps to `via` and runs `x`:
/// the `*` block's `ee`, then its loops' landing prints the table at `x` and jumps to `via`
/// itself; `via` is `x`, or a jump to `x` (the namer joins the other landing's code before the
/// table). Then "zz" with its NUL and `b8` at `x`.
fn a_table_its_namer_runs(joined: bool) -> (Vec<u8>, u32) {
    // jump (5), ee, b5 d2 x b1 (7), jump (5), [jump (5)].
    let via = BODY + 13 + 5 + 1 + 7 + 5;
    let x = if joined { via + 5 } else { via };
    let mut rest = jump(via);
    rest.push(0xee);
    rest.push(0xb5);
    rest.extend(lit32(x));
    rest.push(0xb1);
    rest.extend(jump(via));
    if joined {
        rest.extend(jump(x));
    }
    rest.extend(b"zz\0\xb8");
    (after_an_if(&rest), x)
}

#[test]
fn a_table_never_wins_over_code_its_own_path_runs_joined_or_not() {
    // The namer's path runs `x` as code: through code the a1's landing decoded first (at `x`,
    // or a jump before it). Its landings are dropped, `x` stays code, in either landing order.
    for joined in [false, true] {
        let (s, x) = a_table_its_namer_runs(joined);
        for order in ORDERS {
            let d = in_order(&s, order);
            let what = format!("joined {joined}, order {order}");
            assert!(
                matches!(item(&d, x).kind, Kind::Text(_)),
                "{what}: {:?}",
                shape(&d)
            );
            for scan in [Scan::NoMatch, Scan::Answered] {
                assert!(
                    d.hazards.contains(&Hazard {
                        at: BODY + 19,
                        kind: HazardKind::Dropped(scan, Reason::TableInCode)
                    }),
                    "{what}: {:?}",
                    d.hazards
                );
            }
            assert!(
                !d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))),
                "{what}"
            );
        }
    }
}

#[test]
fn a_path_that_runs_an_else_does_not_run_the_code_an_if_landing_runs_after_it() {
    // A `*` block answering `a1 01 a7 <jump back> a3 "zz" 00 a2 ee`: the a1's loop stops on the
    // `a3` and its landing runs "zz" at `x`. The block's loops print the table at `x` and jump
    // to the `a3`, which they run as an else: it skips to after the `a2`, never into `x`. The
    // table wins and the a1's landing stops there, in either order.
    let (a3, x) = (BODY + 12, BODY + 13);
    let mut body = b"\xf7\xef*\xf6\xa1\x01\xa7".to_vec();
    body.extend(jump(BODY));
    body.extend(b"\xa3zz\0\xa2\xee\xb5");
    body.extend(lit32(x));
    body.push(0xb1);
    body.extend(jump(a3));
    body.push(0xff);
    let s = script(&body);
    let [a, b] = both_orders(&s);
    assert!(a == b, "{:?}\n{:?}", a.1, b.1);
    let d = ok(&s);
    let Kind::Table(t) = &item(&d, x).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(
        t.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [b"zz"]
    );
    assert_eq!(
        d.hazards,
        [Hazard {
            at: x,
            kind: HazardKind::IntoData
        }]
    );
}

/// After [`after_an_if`]'s `a1`, whose landing jumps over "a" to `z`, prints the table `t` at
/// "a" and jumps back: the `*` block's `ee`, then its loops' path prints the string after the
/// one `u` falls in ("ab", past all the code) and jumps back. Returns the script and `x`, `t`,
/// `u`.
fn a_table_over_both_paths() -> (Vec<u8>, [u32; 3]) {
    let x = BODY + 13;
    // jump (5), "a" 00, ee, b5 d2 u 01 a7 (8), jump (5); at z: b5 d2 t b1 (7), jump (5).
    let (t, z) = (x + 5, x + 21);
    let u = z + 13;
    let mut rest = jump(z);
    rest.extend(b"a\0\xee\xb5");
    rest.extend(lit32(u));
    rest.extend([0x01, 0xa7]);
    rest.extend(jump(BODY));
    rest.push(0xb5);
    rest.extend(lit32(t));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend(b"ab\0c\0");
    (after_an_if(&rest), [x, t, u])
}

#[test]
fn a_landing_that_joins_the_op_naming_a_table_names_it_too() {
    // `u` is inside `t`'s strings, so its read keeps `t` up to the string after it: over the
    // a1's landing's code and the block loops'. Both loops run the print from `u`, the one
    // that decoded it and the one that joined it: the three landings are dropped, whichever
    // comes first.
    let (s, [x, _, _]) = a_table_over_both_paths();
    let [a, b] = both_orders(&s);
    assert!(a == b, "{:?}\n{:?}", a.1, b.1);
    let d = ok(&s);
    let mut hazards = d.hazards.clone();
    hazards.sort_by_key(|h| (h.at, format!("{:?}", h.kind)));
    let dropped = |at: u32, scan: Scan| Hazard {
        at,
        kind: HazardKind::Dropped(scan, Reason::TableInCode),
    };
    assert_eq!(
        hazards,
        [
            dropped(x, Scan::If),
            dropped(x + 8, Scan::Answered),
            dropped(x + 8, Scan::NoMatch)
        ]
    );
    assert!(!d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))));
}

#[test]
fn a_landing_that_stops_at_a_table_it_reads_from_inside_is_dropped() {
    // The a1's landing prints the table at `t` = "ab\0cd\0". The block's loops print from `u`,
    // inside its second string, and jump to `t`: taken after the a1's landing they stop there
    // at once, at a byte of a table their path reads; taken first they run "ab" as text. Either
    // way they are dropped, and the table is the a1's landing's.
    let x = BODY + 13;
    let t = x + 25;
    let u = t + 4;
    let mut rest = vec![0xb5];
    rest.extend(lit32(t));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xb5]);
    rest.extend(lit32(u));
    rest.push(0xb1);
    rest.extend(jump(t));
    rest.extend(b"ab\0cd\0");
    let s = after_an_if(&rest);
    let [a, b] = both_orders(&s);
    assert!(a == b, "{:?}\n{:?}", a.1, b.1);
    let d = ok(&s);
    assert_eq!(
        sorted_hazards(&d),
        [
            dropped(x + 13, Scan::Answered, Reason::TableInCode),
            dropped(x + 13, Scan::NoMatch, Reason::TableInCode)
        ]
    );
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.entries.len(), 2);
}

#[test]
fn a_landing_whose_stop_byte_is_in_a_table_it_names_is_dropped() {
    // The a1's loop steps over its jump and an "x" and stops on the `a2` after it; its landing
    // writes word 0 of the table at that "x", whose one word holds the `a2`. The block's loops
    // write the same word. The loop reads the `a2` as a stop byte: a byte of the table its own
    // path names, so the a1's landing is dropped and the table is the loops'.
    let t = BODY + 12;
    let write = [&[0xa6][..], &lit32(t), &[0xb1, 0xa8, 0x07, 0xa7]].concat();
    let mut body = b"\xf7\xef*\xf6\xa1\x01\xa7".to_vec();
    body.extend(jump(BODY));
    body.extend(b"x\xa2");
    body.extend(&write);
    body.extend(jump(BODY));
    body.push(0xee);
    body.extend(&write);
    body.extend(jump(BODY));
    let d = any_order(&script(&body));
    assert_eq!(
        sorted_hazards(&d),
        [dropped(t + 2, Scan::If, Reason::TableInCode)]
    );
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(tb.words);
}

#[test]
fn a_claim_with_a_table_address_in_code_refuses_the_script() {
    // One round of the script where the a1's landing reads "zz" as text and block `q` prints
    // the table there, claimed with no revision (the seam the rounds never leave in a final
    // decode): the address is in that landing's code, and the claim refuses the script at the
    // print, before it scans the range.
    let t = BODY + 13;
    let mut rest = b"zz\0\xee\xefq\xf6\xb5".to_vec();
    rest.extend(lit32(t));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xff]);
    let s = after_an_if(&rest);
    let plan = Plan::default();
    let mut d = Decoder::new(&s, &plan);
    d.run().unwrap();
    assert_eq!(
        d.claim_tables(),
        Err(Refusal {
            at: t + 7,
            reason: Reason::TableInCode
        })
    );
    // With the rounds, the table wins and that landing stops there.
    assert!(matches!(item(&ok(&s), t).kind, Kind::Table(_)));
}

#[test]
fn a_landing_whose_own_path_runs_inside_a_table_it_names_is_dropped() {
    // The `*` block's loops land after its `ee`, print the table at `x` and jump to `x + 3`,
    // the table's second string: their own code inside the table, not at its address.
    let x = BODY + 24;
    let mut tail = vec![0xb5];
    tail.extend(lit32(x));
    tail.push(0xb1);
    tail.extend(jump(x + 3));
    tail.extend(b"ab\0cd\0\xb8");
    let d = ok(&after_a_block(&tail));
    assert!(both_dropped(&d, Reason::TableInCode), "{:?}", d.hazards);
    assert!(!d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))));
}

/// `f7`, a `*` block whose answer's `a1` jumps back when it holds; its false branch (the `a1`'s
/// landing) prints the table right after the block's `ee` and jumps back too. The block's own
/// loops land on that table, whose first byte is one the executor has no arm for. Returns the
/// script and the table's address.
fn a_table_a_failing_landing_reaches() -> (Vec<u8>, u32) {
    // f7 ef * f6 (4), a1 01 a7 (3), jump (5), a2, b5 d2 t b1 (7), jump (5), ee.
    let t = BODY + 4 + 3 + 5 + 1 + 7 + 5 + 1;
    let mut body = b"\xf7\xef*\xf6\xa1\x01\xa7".to_vec();
    body.extend(jump(BODY));
    body.extend([0xa2, 0xb5]);
    body.extend(lit32(t));
    body.push(0xb1);
    body.extend(jump(BODY));
    body.push(0xee);
    body.extend(b"\x80z\0");
    (script(&body), t)
}

#[test]
fn a_landing_that_would_fail_in_a_table_stops_there() {
    // The `a1`'s landing, the lower, names the table first; the block's loops land on it and
    // stop there, in every order the waiting landings are kept in (they would fail on its bytes
    // as code). Failed trials tried again are pinned by
    // `failed_trials_are_tried_again_until_none_more_is_kept`.
    let (s, t) = a_table_a_failing_landing_reaches();
    for order in ORDERS {
        let d = in_order(&s, order);
        assert!(
            matches!(item(&d, t).kind, Kind::Table(_)),
            "order {order}: {:?}",
            shape(&d)
        );
        assert_eq!(
            d.hazards,
            [Hazard {
                at: t,
                kind: HazardKind::IntoData
            }],
            "order {order}"
        );
    }
}

/// After [`after_an_if`]'s `a1`, whose landing reads "abcd" at `x`: the `*` block's `ee`;
/// block `q` (the `*` block's no-match landing) jumps into "abcd" at `x + 2`, splitting the
/// run; block `r` (`q`'s no-match landing) prints the table at `x + 2`. Three landings.
fn a_table_at_a_split_runs_second_part() -> (Vec<u8>, u32) {
    let x = BODY + 13;
    let mut rest = b"abcd\0\xee\xefq\xf6".to_vec();
    rest.extend(jump(x + 2));
    rest.extend(b"\xee\xefr\xf6\xb5");
    rest.extend(lit32(x + 2));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xff]);
    (after_an_if(&rest), x)
}

#[test]
fn a_table_wins_over_the_second_part_of_another_landings_split_run() {
    // The second part of the a1's run keeps the a1's landing as its path: block `r`'s table
    // there wins, and the run's first part and the jump stop there.
    let (s, x) = a_table_at_a_split_runs_second_part();
    let d = ok(&s);
    let Kind::Table(t) = &item(&d, x + 2).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(
        t.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [b"cd"]
    );
    assert!(matches!(item(&d, x).kind, Kind::Text(_)) && item(&d, x).len == 2);
    assert!(
        d.hazards.contains(&Hazard {
            at: x + 2,
            kind: HazardKind::IntoData
        }),
        "{:?}",
        d.hazards
    );
    assert!(
        !d.hazards
            .iter()
            .any(|h| matches!(h.kind, HazardKind::Dropped(..))),
        "{:?}",
        d.hazards
    );
}

/// `f7`, a `*` block whose answer is `a1 01 a7 a3`: the `a1`'s landing (after the `a3`) jumps
/// into table `u`; the `a3`'s landing (after the `a2`) prints `u` and jumps into table `t`;
/// the block's own loops print `t` and jump back. Both tables start with a byte the executor
/// has no arm for. Returns the script, `t` and `u`.
fn tables_each_landing_needs_kept_first() -> (Vec<u8>, u32, u32) {
    // f7 ef * f6 (4), a1 01 a7 (3), a3, jump (5), a2, b5 d2 u b1 (7), jump (5), ee,
    // b5 d2 t b1 (7), jump (5).
    let t = BODY + 4 + 3 + 1 + 5 + 1 + 7 + 5 + 1 + 7 + 5;
    let u = t + 3;
    let mut body = b"\xf7\xef*\xf6\xa1\x01\xa7\xa3".to_vec();
    body.extend(jump(u));
    body.extend([0xa2, 0xb5]);
    body.extend(lit32(u));
    body.push(0xb1);
    body.extend(jump(t));
    body.extend([0xee, 0xb5]);
    body.extend(lit32(t));
    body.push(0xb1);
    body.extend(jump(BODY));
    body.extend(b"\x80a\0\x80b\0");
    (script(&body), t, u)
}

#[test]
fn failed_trials_are_tried_again_until_none_more_is_kept() {
    // In script order the `a1`'s landing fails in `u` and the `a3`'s in `t`; the block's loops
    // keep `t`. Tried again, the `a1`'s landing still fails (nothing keeps `u` yet), the `a3`'s
    // stops at `t` and keeps `u`; tried once more, the `a1`'s stops at `u`. Either order.
    let (s, t, u) = tables_each_landing_needs_kept_first();
    for order in ORDERS {
        let d = in_order(&s, order);
        for at in [t, u] {
            assert!(
                matches!(item(&d, at).kind, Kind::Table(_)),
                "order {order}: {:?}",
                shape(&d)
            );
        }
        let mut hazards = d.hazards.clone();
        hazards.sort_by_key(|h| h.at);
        assert_eq!(
            hazards,
            [
                Hazard {
                    at: t,
                    kind: HazardKind::IntoData
                },
                Hazard {
                    at: u,
                    kind: HazardKind::IntoData
                }
            ],
            "order {order}"
        );
    }
}

/// The orders the tests keep the waiting landings in ([`ORDER`]): as found, the reverse, and
/// two shuffles.
const ORDERS: [u64; 4] = [0, 1, 0x5eed, 0xc0ffee];

/// The decode of `s` with the waiting landings kept in `order`.
fn in_order(s: &[u8], order: u64) -> Disasm {
    ORDER.with(|o| o.set(order));
    let d = ok(s);
    ORDER.with(|o| o.set(0));
    d
}

/// The decode of `s` with the waiting landings kept in `order`: its items, its hazards (as a
/// set) and its catalog (it decodes again: in the same order).
fn with_order(s: &[u8], order: u64) -> (Vec<Item>, Vec<String>, String) {
    ORDER.with(|o| o.set(order));
    let d = ok(s);
    let catalog = Catalog::build([(b'a', 7, s)]).to_json();
    ORDER.with(|o| o.set(0));
    let mut hazards: Vec<String> = d.hazards.iter().map(|h| format!("{h:?}")).collect();
    hazards.sort();
    (d.items, hazards, catalog)
}

/// [`with_order`] as found and in the reverse order.
fn both_orders(s: &[u8]) -> [(Vec<Item>, Vec<String>, String); 2] {
    [0, 1].map(|order| with_order(s, order))
}

/// The decode of `s`, the same in every one of [`ORDERS`] (items, hazards, catalog).
fn any_order(s: &[u8]) -> Disasm {
    let first = with_order(s, ORDERS[0]);
    for order in &ORDERS[1..] {
        let other = with_order(s, *order);
        assert!(
            first == other,
            "order {order}: {:?}\n{:?}",
            first.1,
            other.1
        );
    }
    ok(s)
}

#[test]
fn which_of_a_table_and_a_landings_code_wins_does_not_depend_on_the_landing_order() {
    // Scripts where a table meets a landing's code: each decodes the same, catalog included,
    // with the waiting landings kept in the order found and in the reverse.
    let mut scripts = vec![
        a_table_its_namer_runs(false).0,
        a_table_its_namer_runs(true).0,
        a_table_at_an_ifs_code(&[0xb1], b"zz\xb6").0,
        a_table_at_an_ifs_code(&[0x01, 0xb2, 0xa7], b"zz\0").0,
        a_read_into_the_next_table(b"").0,
    ];
    let mut later = b"\xefj\xf6A".to_vec();
    later.extend(jump(BODY));
    later.push(0xee);
    scripts.push(a_read_into_the_next_table(&later).0);
    let t = BODY + 13;
    let mut rest = b"zz\0\xee\xefq\xf6\xb5".to_vec();
    rest.extend(lit32(t));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xff]);
    scripts.push(after_an_if(&rest));
    scripts.push(a_table_a_failing_landing_reaches().0);
    scripts.push(a_table_at_a_split_runs_second_part().0);
    scripts.push(tables_each_landing_needs_kept_first().0);
    // A run split by a jump into it: its parts are linked in the catalog.
    let mut body = b"Hi there\xf7\xef*\xf6".to_vec();
    body.extend(jump(BODY + 3));
    body.extend([0xee, 0xff]);
    scripts.push(script(&body));
    for (n, s) in scripts.iter().enumerate() {
        let [a, b] = both_orders(s);
        assert!(a == b, "script {n}: {:?}\n{:?}", a.1, b.1);
    }
}

/// A script of random pieces after the greeting: keyword blocks, `a1` / `a3` / `a2`, jumps,
/// prints and copies of table addresses (a literal, a constant or computed index), searches,
/// word writes and reads, text, NULs and `b8`s; a 64-bit LCG with seed `g`. Many landings, and
/// many tables over their code.
fn random_pieces(g: &mut u64) -> Vec<u8> {
    let mut next = |n: u32| {
        *g = g
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*g >> 33) as u32 % n
    };
    let mut b = b"\xf7".to_vec();
    for _ in 0..6 + next(14) {
        let a = BODY + next(90);
        let piece: Vec<u8> = match next(20) {
            0 => b"ab".to_vec(),
            1 => b"zz\0".to_vec(),
            2 => vec![0xee],
            3 => b"\xefk\xf6".to_vec(),
            4 => vec![0xa1, 0x01, 0xa7],
            5 => vec![0xa2],
            6 => vec![0xa3],
            7 => jump(a),
            8 => [&[0xb5][..], &lit32(a), &[0xb1]].concat(),
            9 => [&[0xb5][..], &lit32(a), &[0x01, 0xb2, 0xa7]].concat(),
            10 => vec![0xb6],
            11 => vec![0],
            12 => b"cd\0ef\0".to_vec(),
            13 => jump(BODY),
            14 => [&[0xb5][..], &lit32(a), &[0x01, 0xa7]].concat(),
            15 => search_of(a, false),
            16 => [&[0xa6][..], &lit32(a), &[0xb1, 0xa8, 0x07, 0xa7]].concat(),
            17 => [
                &[0xa6, 0x01, 0xb2, 0xa8][..],
                &lit32(a),
                &[0x00, 0xb4, 0xa7],
            ]
            .concat(),
            18 => vec![0x10, 0x00, 0x20, 0x00, 0xb8],
            _ => b"\xefq\xf6".to_vec(),
        };
        b.extend(piece);
    }
    b.push(0xff);
    script(&b)
}

#[test]
fn the_catalog_is_the_same_in_any_order_the_landings_are_found_in() {
    // Random scripts with many landings and tables, each decoded with its waiting landings
    // kept in several orders: the same decode and the same catalog, byte for byte.
    let mut g = 0x0d15_a55e_u64;
    let (mut decoded, mut landings) = (0, 0);
    for _ in 0..3000 {
        let s = random_pieces(&mut g);
        let build = |order: u64| {
            ORDER.with(|o| o.set(order));
            let d = disassemble(b'a', 7, &s);
            let c = Catalog::build([(b'a', 7, s.as_slice())]).to_json();
            ORDER.with(|o| o.set(0));
            (d, c)
        };
        let (first, catalog) = build(0);
        for order in [1, 0x5eed, 0xc0ffee, 0xbad5eed] {
            let (d, c) = build(order);
            assert!(d == first && c == catalog, "order {order}: {s:02x?}");
        }
        if let Ok(d) = first {
            decoded += 1;
            landings += d
                .hazards
                .iter()
                .filter(|h| matches!(h.kind, HazardKind::Dropped(..) | HazardKind::IntoData))
                .count();
        }
    }
    // The scripts decode, and their landings meet tables and each other.
    assert!(decoded > 2000 && landings > 150, "{decoded} {landings}");
}

/// `d`'s hazards, sorted.
fn sorted_hazards(d: &Disasm) -> Vec<Hazard> {
    let mut h = d.hazards.clone();
    h.sort_by_key(|h| (h.at, format!("{:?}", h.kind)));
    h
}

fn dropped(at: u32, scan: Scan, reason: Reason) -> Hazard {
    Hazard {
        at,
        kind: HazardKind::Dropped(scan, reason),
    }
}

// Conflicts between landings are resolved in ascending landing address (the module doc): the
// tests below show three kinds, each the same in every order the landings are found in.

#[test]
fn of_two_landings_whose_code_disagrees_on_a_byte_the_lower_keeps_it() {
    // The a1's landing (at `x`) jumps into the operand of the jump the block's loops (at
    // `x + 6`) run after its `ee`, and reads a newline there: the a1's landing keeps it.
    let x = BODY + 13;
    let mut rest = jump(x + 7);
    rest.push(0xee);
    rest.extend(jump(BODY));
    let d = any_order(&after_an_if(&rest));
    assert_eq!(
        sorted_hazards(&d),
        [
            dropped(x + 6, Scan::Answered, Reason::Overlap),
            dropped(x + 6, Scan::NoMatch, Reason::Overlap)
        ]
    );
    assert!(matches!(item(&d, x + 7).kind, Kind::Text(_)));
    // The loop does not decide: here the `*` block's loops land at `BODY + 10` and run a jump
    // back; the `a1`, in the code the block's answer jumps to, lands after its `a2` and jumps
    // into that jump's operand. The loops' landing is the lower: it keeps its jump.
    let p = BODY + 15;
    let mut body = b"\xf7\xef*\xf6".to_vec();
    body.extend(jump(p));
    body.push(0xee);
    body.extend(jump(BODY));
    body.extend([0xa1, 0x01, 0xa7]);
    body.extend(jump(BODY));
    body.push(0xa2);
    body.extend(jump(BODY + 11));
    let d = any_order(&script(&body));
    assert_eq!(
        sorted_hazards(&d),
        [dropped(p + 9, Scan::If, Reason::Overlap)]
    );
    assert!(matches!(op_at(&d, BODY + 10).info().flow, Flow::Jump));
}

#[test]
fn a_lower_landing_only_a_higher_ones_path_finds_comes_after_it() {
    // The `*` block's loops land at `h` and jump to an `a1` only their path reaches; its
    // landing at `l`, lower than `h`, is found only then, and jumps into the operand of the
    // loops' jump. The loops were taken first: they keep their jump, the `a1`'s landing is
    // dropped.
    let a1 = BODY + 11;
    let l = a1 + 9;
    let h = l + 6;
    let mut b = b"\xf7\xef*\xf6Ok".to_vec();
    b.extend(jump(BODY));
    b.extend([0xa1, 0x01, 0xa7]);
    b.extend(jump(BODY));
    b.push(0xa2);
    b.extend(jump(h + 2));
    b.push(0xee);
    b.extend(jump(a1));
    b.push(0xff);
    let d = any_order(&script(&b));
    assert_eq!(drops(&d), [dropped(l, Scan::If, Reason::Overlap)]);
    assert!(matches!(op_at(&d, h).info().flow, Flow::Jump));
}

#[test]
fn of_two_landings_reading_one_address_with_two_kinds_the_lower_keeps_it() {
    // The a1's landing writes word 0 of `t`; the block's loops, at a higher address, print
    // string 0 of `t`: `t` is a word table and the loops are dropped.
    let t = BODY + 13 + 10 + 5 + 1 + 7 + 5;
    let mut rest = vec![0xa6];
    rest.extend(lit32(t));
    rest.extend([0xb1, 0xa8, 0x07, 0xa7]);
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xb5]);
    rest.extend(lit32(t));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend(b"zz\0\xb8");
    let d = any_order(&after_an_if(&rest));
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(tb.words);
    let ee = BODY + 13 + 10 + 5;
    assert_eq!(
        sorted_hazards(&d),
        [
            dropped(ee + 1, Scan::Answered, Reason::TableKinds),
            dropped(ee + 1, Scan::NoMatch, Reason::TableKinds)
        ]
    );
}

#[test]
fn a_landings_code_at_a_table_another_landing_names_is_decided_by_their_addresses() {
    let x = BODY + 13;
    // Each of two landings names a table over the other's code: the a1's landing (at `x`)
    // prints the table at `y`, where the block's loops land; they print the table at `x + 2`,
    // inside the a1's landing's print. The lower keeps its table; the other stops there.
    let y = x + 13;
    let mut rest = vec![0xb5];
    rest.extend(lit32(y));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xb5]);
    rest.extend(lit32(x + 2));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    let d = any_order(&after_an_if(&rest));
    assert!(
        matches!(item(&d, y).kind, Kind::Table(_)),
        "{:?}",
        shape(&d)
    );
    assert!(matches!(item(&d, x).kind, Kind::Op(_)));
    assert_eq!(
        sorted_hazards(&d),
        [Hazard {
            at: y,
            kind: HazardKind::IntoData
        }]
    );
    // One landing names a table over the other's code (string 1 of `t` holds the block's `ee`
    // and the start of its loops' jump, `u` is in that jump's operand). The namer is the
    // lower: the loops' jump would hold `u`, and they are dropped.
    let (t, u) = (x + 20, x + 26);
    let mut rest = vec![0xb5];
    rest.extend(lit32(t));
    rest.extend([0x01, 0xa7, 0xb5]);
    rest.extend(lit32(u));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend(b"a\0\xee");
    rest.extend(jump(BODY));
    let d = any_order(&after_an_if(&rest));
    assert!(matches!(item(&d, t).kind, Kind::Table(_)));
    assert!(matches!(item(&d, u).kind, Kind::Table(_)));
    assert_eq!(
        sorted_hazards(&d),
        [
            dropped(x + 23, Scan::Answered, Reason::TableInCode),
            dropped(x + 23, Scan::NoMatch, Reason::TableInCode)
        ]
    );
    // A script from the random order probe (synthetic bytes): the a1's landing at 61 (its loop
    // stops on the `a2` at 60) jumps back into the direct code, whose print at 16 names the
    // table at 58. Block `k`'s landing (at 75) prints from 60, inside that table's second
    // string: the table keeps it, over the a1's landing's code. The a1's landing is the lower:
    // its code is in, in a table its path names, and it is dropped.
    let s = script(&[
        0xf7, 0x61, 0x62, 0xef, 0x71, 0xf6, 0xb5, 0xd2, 0x3a, 0x00, 0x00, 0x00, 0xb1, 0xef, 0x71,
        0xf6, 0xb5, 0xd2, 0x26, 0x00, 0x00, 0x00, 0xb1, 0xa1, 0x01, 0xa7, 0xb6, 0xb5, 0xd2, 0x19,
        0x00, 0x00, 0x00, 0x01, 0xb2, 0xa7, 0xb5, 0xd2, 0x4d, 0x00, 0x00, 0x00, 0xb1, 0xb5, 0xd2,
        0x4f, 0x00, 0x00, 0x00, 0xb1, 0xa2, 0xb0, 0x0a, 0x00, 0x00, 0x00, 0x7a, 0x7a, 0x00, 0x63,
        0x64, 0x00, 0x65, 0x66, 0x00, 0xef, 0x6b, 0xf6, 0xb5, 0xd2, 0x3c, 0x00, 0x00, 0x00, 0xb1,
        0x00, 0x61, 0x62, 0xff,
    ]);
    let d = any_order(&s);
    assert!(
        d.hazards
            .contains(&dropped(61, Scan::If, Reason::TableInCode))
    );
}

#[test]
fn an_address_a_later_round_no_longer_names_is_settled_and_its_namer_dropped() {
    // The `a1`'s landing reads "zz" at BODY + 13 as text and jumps on to block `q` (reached no
    // other way: the `*` block's no-match loop stops at the `ff`, its answered loop on an `ee`
    // inside the jump's operand); block `q`'s skip loops reach a print of the table at
    // BODY + 13. Closing it stops the `a1`'s landing there, so block `q` and the print are not
    // reached and nothing names it: it is opened again, and the print's path is dropped.
    let (t, q) = (BODY + 13, BODY + 21);
    let mut rest = b"zz\xb0".to_vec();
    rest.extend([q as u8, 0x00, 0xee, 0x00, 0xff]);
    rest.extend(b"\xefq\xf6A");
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xb5]);
    rest.extend(lit32(t));
    rest.extend([0xb1, 0xb6]);
    let s = after_an_if(&rest);
    let d = ok(&s);
    assert!(matches!(item(&d, t).kind, Kind::Text(_)), "{:?}", shape(&d));
    assert!(matches!(item(&d, q).kind, Kind::Keywords(_)));
    assert!(!d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))));
    for scan in [Scan::NoMatch, Scan::Answered] {
        assert!(
            d.hazards.contains(&Hazard {
                at: q + 10,
                kind: HazardKind::Dropped(scan, Reason::TableInCode)
            }),
            "{:?}",
            d.hazards
        );
    }
    assert!(!d.hazards.iter().any(|h| h.kind == HazardKind::IntoData));
}

#[test]
fn a_settled_byte_is_never_reserved_again() {
    // The `a1`'s landing reads "zz" and its NUL in the table block `q` prints. With the NUL's
    // byte settled before this round, the table wins over "zz" (its bytes are reserved) but not
    // over the NUL's stop (block `q`'s landings are dropped): the settled byte stays open.
    let t = BODY + 13;
    let mut rest = b"zz\0\xee\xefq\xf6\xb5".to_vec();
    rest.extend(lit32(t));
    rest.push(0xb1);
    rest.extend(jump(BODY));
    rest.extend([0xee, 0xff]);
    let s = after_an_if(&rest);
    let plan = Plan::settled([t + 2]);
    let mut d = Decoder::new(&s, &plan);
    d.run().unwrap();
    let next = d.revise().expect("another round");
    let (reserved, settled) = next.bytes();
    assert_eq!(settled.iter().copied().collect::<Vec<_>>(), [t + 2]);
    assert_eq!(reserved.iter().copied().collect::<Vec<_>>(), [t, t + 1]);
}

#[test]
fn a_constant_index_past_a_landing_paths_table_drops_that_path() {
    // Only the final tables tell an index past its table: the next round drops the landing.
    let t = BODY + 21;
    let mut tail = vec![0xb5];
    tail.extend(lit32(t));
    tail.extend([0x05, 0xa7, 0xff]);
    tail.extend(b"a\0b\0");
    let d = ok(&after_a_block(&tail));
    assert!(both_dropped(&d, Reason::TableIndex), "{:?}", d.hazards);
    assert!(!d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))));
}

#[test]
fn a_read_of_a_table_with_no_entry_reads_past_it() {
    // At `x`, "zz" and the script's end, no NUL and no `b8`: a print with a computed index, or
    // a search, reads on past the script. A direct path's read refuses the script.
    let print = |x: u32| [&[0xb5][..], &lit32(x), &[0x01, 0xb2, 0xa7]].concat();
    let search = |x: u32| {
        [
            &[0xa6, 0x01, 0xb2, 0xa8][..],
            &lit32(x),
            &[0x03, 0xb7, 0xa7],
        ]
        .concat()
    };
    let with = |op: &dyn Fn(u32) -> Vec<u8>, tail: &[u8]| {
        let x = BODY + op(0).len() as u32 + 1;
        let mut body = op(x);
        body.push(0xb6);
        body.extend_from_slice(tail);
        (script(&body), x)
    };
    for (what, op) in [
        ("print", &print as &dyn Fn(u32) -> Vec<u8>),
        ("search", &search),
    ] {
        let (s, _) = with(op, b"zz");
        assert_eq!(
            refused(&s),
            Refusal {
                at: BODY,
                reason: Reason::TableIndex
            },
            "{what}"
        );
    }
    // A `b8` at the address ends a search there; a print reads past it.
    let (s, x) = with(&search, b"\xb8");
    let d = ok(&s);
    let Kind::Table(t) = &item(&d, x).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(t.terminated && t.entries.is_empty());
    assert_eq!(refused(&with(&print, b"\xb8").0).reason, Reason::TableIndex);
    // On a landing's path, the landing is dropped.
    let x = BODY + 12 + 10;
    let mut tail = print(x);
    tail.push(0xb6);
    tail.extend(b"zz");
    let d = ok(&after_a_block(&tail));
    assert!(both_dropped(&d, Reason::TableIndex), "{:?}", d.hazards);
}

/// `a6 01 b2 a8 d2 <x> 03 b7 a7` (`b7`: a search of the strings at `x` for slot 3), or with
/// `01 c3` (`c3`: of the words at `x` for word variable 1).
fn search_of(x: u32, words: bool) -> Vec<u8> {
    let op: &[u8] = if words { &[0x01, 0xc3] } else { &[0x03, 0xb7] };
    [&[0xa6, 0x01, 0xb2, 0xa8][..], &lit32(x), op, &[0xa7]].concat()
}

#[test]
fn a_search_reads_on_past_a_table_with_no_b8_as_the_interpreter_does() {
    // A direct search of "ab\0cd\0zz" at `t`, then the script's end: with no entry matching,
    // the interpreter reads "zz" and past the end. A hazard at the op, with those bytes.
    let with = |op: Vec<u8>, tail: &[u8]| {
        let t = BODY + op.len() as u32 + 1;
        let mut body = op;
        body.push(0xb6);
        body.extend_from_slice(tail);
        (script(&body), t)
    };
    let past = |d: &Disasm| -> Vec<Span> {
        d.hazards
            .iter()
            .filter_map(|h| match h.kind {
                HazardKind::SearchPastEnd(sp) if h.at == BODY => Some(sp),
                _ => None,
            })
            .collect()
    };
    let (s, t) = with(search_of(BODY + 13, false), b"ab\0cd\0zz");
    let d = ok(&s);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(
        tb.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [&b"ab"[..], b"cd"]
    );
    assert_eq!(
        past(&d),
        [Span {
            offset: t + 6,
            len: 2
        }]
    );
    // Its last entry at the script's end; a word search with one byte left.
    let (s, t) = with(search_of(BODY + 13, false), b"ab\0");
    assert_eq!(
        past(&ok(&s)),
        [Span {
            offset: t + 3,
            len: 0
        }]
    );
    let (s, t) = with(search_of(BODY + 13, true), b"\x10\x00\x20\x00\x30");
    assert_eq!(
        past(&ok(&s)),
        [Span {
            offset: t + 4,
            len: 1
        }]
    );
    // A search from inside the first string of the table a print names.
    let t = BODY + 30;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.push(0xb1);
    body.extend(search_of(t + 1, false));
    body.push(0xb6);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"abcd\0ef\0");
    let d = ok(&script(&body));
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!((tb.inner.as_slice(), tb.entries.len()), (&[t + 1][..], 2));
    assert!(d.hazards.contains(&Hazard {
        at: BODY + 7,
        kind: HazardKind::SearchPastEnd(Span {
            offset: t + 8,
            len: 0
        })
    }));
    // A `b8` ends it: no hazard.
    let (s, _) = with(search_of(BODY + 13, false), b"ab\0\xb8");
    assert!(past(&ok(&s)).is_empty());
}

#[test]
fn a_search_stops_at_a_b8_that_starts_the_next_table_of_either_kind() {
    // Searches of `t` = "ab\0" and of `t + 3`, the `b8` after it (an empty terminated table of
    // the same kind): the search of `t` reads "ab", then that `b8` at an entry's start, and
    // stops. A word search of the one word at `t` and a string search of the `b8` after it (a
    // table of the other kind): the word search reads the word, then the `b8`, and stops. No
    // read past a table, no hazard; on a landing's path no landing is dropped.
    let two = |first: Vec<u8>, second: Vec<u8>, tail: &[u8]| {
        let t = BODY + first.len() as u32 + second.len() as u32 + 1;
        let mut body = first;
        body.extend(second);
        body.push(0xb6);
        body.extend_from_slice(tail);
        (body, t)
    };
    let t = BODY + 25;
    let (body, t1) = two(search_of(t, false), search_of(t + 3, false), b"ab\0\xb8");
    assert_eq!(t1, t);
    let d = any_order(&script(&body));
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
    let (body, _) = two(search_of(t, true), search_of(t + 2, false), b"\x10\x00\xb8");
    let d = any_order(&script(&body));
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
    let Kind::Table(words) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(words.words && !words.terminated && words.entries.len() == 1);
    // A table of the other kind that does not start with a `b8`: the word search reads its
    // bytes, past its own table.
    let (body, _) = two(
        search_of(t, true),
        search_of(t + 2, false),
        b"\x10\x00ab\0\xb8",
    );
    assert_eq!(refused(&script(&body)).reason, Reason::TableIndex);
    // The first two searches on a landing's path.
    let t = BODY + 12 + 25;
    let mut tail = search_of(t, false);
    tail.extend(search_of(t + 3, false));
    tail.push(0xb6);
    tail.extend(b"ab\0\xb8");
    let d = any_order(&after_a_block(&tail));
    assert!(
        !d.hazards
            .iter()
            .any(|h| matches!(h.kind, HazardKind::Dropped(..))),
        "{:?}",
        d.hazards
    );
}

#[test]
fn a_search_from_a_b8_inside_a_string_reads_nothing() {
    // A print of `t` = "ab\xb8cd\0" and a search from `t + 2`, the `b8` inside its string: the
    // interpreter reads that `b8` at its first entry's start and stops. With direct code after
    // the table, with the script's end after it ("zz" no item holds), and on a landing's path:
    // no read past the table, no hazard, and the table is only the string the print reads.
    let with = |base: u32, tail: &[u8], code_after: bool| {
        let t = base + 7 + 12 + if code_after { 5 } else { 1 };
        let mut b = vec![0xb5];
        b.extend(lit32(t));
        b.push(0xb1);
        b.extend(search_of(t + 2, false));
        if code_after {
            b.extend(jump(t + 6));
        } else {
            b.push(0xb6);
        }
        b.extend(b"ab\xb8cd\0");
        b.extend_from_slice(tail);
        (b, t)
    };
    let (body, t) = with(BODY, b"\xb6", true);
    let d = any_order(&script(&body));
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!((tb.entries.len(), tb.inner.as_slice()), (1, &[t + 2][..]));
    let (body, t) = with(BODY, b"zz", false);
    let d = any_order(&script(&body));
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.entries.len(), 1);
    let (tail, _) = with(BODY + 12, b"\xb6", true);
    let d = any_order(&after_a_block(&tail));
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
}

/// The `*` block's answer (direct code) makes `op(t)` and jumps back; after its `ee` its loops
/// write word 0 at `t + 3` (or, `!inside`, at `BODY`, direct code) and jump back; then `t` =
/// "ab\0cd\0" to the script's end. Returns the script, `t` and the loops' landing.
fn a_word_write_after_a_direct_read(
    op: &dyn Fn(u32) -> Vec<u8>,
    inside: bool,
) -> (Vec<u8>, u32, u32) {
    let n = op(0).len() as u32;
    let at = BODY + 4 + n + 5 + 1;
    let t = at + 10 + 5;
    let mut b = b"\xf7\xef*\xf6".to_vec();
    b.extend(op(t));
    b.extend(jump(BODY));
    b.extend([0xee, 0xa6]);
    b.extend(lit32(if inside { t + 3 } else { BODY }));
    b.extend([0xb1, 0xa8, 0x07, 0xa7]);
    b.extend(jump(BODY));
    b.extend(b"ab\0cd\0");
    (script(&b), t, at)
}

#[test]
fn a_landings_word_table_inside_a_direct_string_table_drops_the_landing() {
    // The direct read is string 1 of `t` (a constant index) or a search of `t`: it keeps both
    // strings, and the loops' word write at `t + 3`, the start of the second, is inside them.
    // The direct table wins: it keeps both strings, the loops are dropped (one byte read as a
    // word and as a string). Aimed at direct code instead, the write drops them as code.
    let index1 = |t: u32| [&[0xb5][..], &lit32(t), &[0x01, 0xa7]].concat();
    let search = |t: u32| search_of(t, false);
    for (what, op) in [
        ("index 1", &index1 as &dyn Fn(u32) -> Vec<u8>),
        ("search", &search),
    ] {
        for inside in [true, false] {
            let (s, t, at) = a_word_write_after_a_direct_read(op, inside);
            let what = format!("{what}, write inside {inside}");
            let d = any_order(&s);
            let Kind::Table(tb) = &item(&d, t).kind else {
                panic!("{what}: {:?}", shape(&d))
            };
            assert_eq!(
                tb.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
                [&b"ab"[..], b"cd"],
                "{what}"
            );
            let reason = if inside {
                Reason::TableKinds
            } else {
                Reason::TableInCode
            };
            let loops: Vec<Hazard> = sorted_hazards(&d)
                .into_iter()
                .filter(|h| matches!(h.kind, HazardKind::Dropped(..)))
                .collect();
            assert_eq!(
                loops,
                [
                    dropped(at, Scan::Answered, reason),
                    dropped(at, Scan::NoMatch, reason)
                ],
                "{what}"
            );
            assert!(
                !d.items
                    .iter()
                    .any(|i| matches!(&i.kind, Kind::Table(t) if t.words))
            );
        }
    }
    // The direct read is string 0 only: the word at `t + 3` is past what it reads, so it ends
    // the direct table there and the loops are kept.
    let index0 = |t: u32| [&[0xb5][..], &lit32(t), &[0xb1]].concat();
    let (s, t, _) = a_word_write_after_a_direct_read(&index0, true);
    let d = any_order(&s);
    assert!(
        !d.hazards
            .iter()
            .any(|h| matches!(h.kind, HazardKind::Dropped(..))),
        "{:?}",
        d.hazards
    );
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.entries.len(), 1);
    assert!(matches!(&item(&d, t + 3).kind, Kind::Table(w) if w.words));
}

/// After [`after_an_if`]'s `a1`, whose landing jumps to the search `S` of `t` = "ab\0cd\0"
/// (no `b8`, the script's end after it): the `*` block's `ee`, then its loops write word 0 at
/// `t + 3` and run on into `S`. Returns the script, the a1's landing, the loops' landing and
/// `S`'s offset.
fn a_search_two_landings_run() -> (Vec<u8>, [u32; 3]) {
    let x = BODY + 13;
    let s_at = x + 5 + 1 + 10;
    let t = s_at + 12 + 5;
    let mut rest = jump(s_at);
    rest.extend([0xee, 0xa6]);
    rest.extend(lit32(t + 3));
    rest.extend([0xb1, 0xa8, 0x07, 0xa7]);
    rest.extend(search_of(t, false));
    rest.extend(jump(BODY));
    rest.extend(b"ab\0cd\0");
    (after_an_if(&rest), [x, x + 6, s_at])
}

/// One round of `s` under `plan` up to its read-past check: the next plan, if any.
fn index_round(s: &[u8], plan: &Plan) -> Option<Plan> {
    let mut d = Decoder::new(s, plan);
    d.run().unwrap();
    assert!(d.revise().is_none(), "a round that revises");
    let pre = d.clone();
    d.claim_tables().unwrap();
    d.check_table_indices(&pre).unwrap()
}

#[test]
fn a_read_past_a_table_bans_every_landing_that_runs_it_and_lifts_when_its_cause_is_gone() {
    // The loops' word table at `t + 3` cuts `t`, so `S` reads on onto it. The a1's landing
    // (the lower) decodes `S`, the loops join it: all three landings are banned for it. With
    // them banned, the a1's landing's own path reads only strings: its ban lifts; the loops'
    // path still makes the word table that cuts `S`: theirs stays. The a1's landing is kept,
    // its search reads on past the script's end.
    let (s, [x, loops, s_at]) = a_search_two_landings_run();
    let first = index_round(&s, &Plan::default()).expect("bans");
    let (banned, lifted) = first.bans();
    let parts: Vec<(Scan, u32, Reason)> = banned
        .iter()
        .map(|(k, &r)| (k.parts().0, k.parts().1, r))
        .collect();
    assert_eq!(
        parts,
        [
            (Scan::If, x - 1, Reason::TableIndex),
            (Scan::NoMatch, loops - 1, Reason::TableIndex),
            (Scan::Answered, loops - 1, Reason::TableIndex)
        ]
    );
    assert!(lifted.is_empty());
    let second = index_round(&s, &first).expect("a lift");
    let (banned, lifted) = second.bans();
    assert_eq!(
        banned.keys().map(|k| k.parts().0).collect::<Vec<_>>(),
        [Scan::NoMatch, Scan::Answered]
    );
    assert_eq!(
        lifted.iter().map(|k| k.parts().0).collect::<Vec<_>>(),
        [Scan::If]
    );
    let d = any_order(&s);
    assert_eq!(
        sorted_hazards(&d),
        [
            dropped(loops, Scan::Answered, Reason::TableIndex),
            dropped(loops, Scan::NoMatch, Reason::TableIndex),
            Hazard {
                at: s_at,
                kind: HazardKind::SearchPastEnd(Span {
                    offset: d.len,
                    len: 0
                })
            }
        ]
    );
    assert!(!d.hazards.iter().any(|h| h.at == x));
    // A ban lifted once and made again stays: with the a1's landing's marked lifted before,
    // nothing more lifts.
    let again = first
        .clone()
        .lifted_before(*second.bans().1.iter().next().unwrap());
    assert_eq!(index_round(&s, &again), None);
}

#[test]
fn of_two_landings_each_reading_past_the_others_table_the_lower_is_kept() {
    // The a1's landing (at `x`) prints string 1 of `ta` and writes a word inside `tb`'s second
    // string; the block's loops (higher) do the mirror. Each one's word table cuts the other's
    // string table, so each one's read past comes from the other: all are dropped for it. With
    // them out, the a1's landing's ban lifts first; tried with it in, the loops' cause is still
    // there, and they stay dropped.
    let print1 = |t: u32| [&[0xb5][..], &lit32(t), &[0x01, 0xa7]].concat();
    let x = BODY + 13;
    let loops = x + 24;
    let ta = loops + 23;
    let tb = ta + 6;
    let mut rest = print1(ta);
    rest.extend(write_word(tb + 3, 0));
    rest.extend(jump(BODY));
    rest.push(0xee);
    rest.extend(print1(tb));
    rest.extend(write_word(ta + 3, 0));
    rest.extend(jump(BODY));
    rest.extend(b"ab\0cd\0ef\0gh\0");
    let d = any_order(&after_an_if(&rest));
    assert_eq!(
        drops(&d),
        [
            dropped(loops, Scan::Answered, Reason::TableIndex),
            dropped(loops, Scan::NoMatch, Reason::TableIndex)
        ]
    );
    // The a1's landing's print of string 1 reads its table.
    let Kind::Table(t) = &item(&d, ta).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(t.entries.len() >= 2);
}

#[test]
fn scripts_whose_landing_order_decided_a_read_past_a_table_decode_one_way() {
    // Scripts from a random probe of the decoder before landings were taken in a fixed order
    // (synthetic bytes): in each, the two orders gave two decodes, one through a read past a
    // table. They decode, and to the same catalog, in every order.
    let residues: [&[u8]; 5] = [
        &[
            0xf7, 0xef, 0x71, 0xf6, 0xa1, 0x01, 0xa7, 0xa3, 0xb5, 0xd2, 0x47, 0x00, 0x00, 0x00,
            0x01, 0xb2, 0xa7, 0xb5, 0xd2, 0x3e, 0x00, 0x00, 0x00, 0x01, 0xb2, 0xa7, 0x00, 0xb0,
            0x0a, 0x00, 0x00, 0x00, 0xb6, 0xb5, 0xd2, 0x0e, 0x00, 0x00, 0x00, 0xb1, 0xb0, 0x0a,
            0x00, 0x00, 0x00, 0xa2, 0xef, 0x6b, 0xf6, 0xef, 0x6b, 0xf6, 0xa2, 0xb5, 0xd2, 0x22,
            0x00, 0x00, 0x00, 0x01, 0xb2, 0xa7, 0xff,
        ],
        &[
            0xf7, 0xef, 0x6b, 0xf6, 0x61, 0x62, 0xb0, 0x0a, 0x00, 0x00, 0x00, 0xb0, 0x3c, 0x00,
            0x00, 0x00, 0xef, 0x71, 0xf6, 0xa6, 0x01, 0xb2, 0xa8, 0xd2, 0x31, 0x00, 0x00, 0x00,
            0x00, 0xb4, 0xa7, 0xef, 0x71, 0xf6, 0x10, 0x00, 0x20, 0x00, 0xb8, 0xa3, 0xee, 0xb5,
            0xd2, 0x24, 0x00, 0x00, 0x00, 0x01, 0xb2, 0xa7, 0xef, 0x6b, 0xf6, 0xb5, 0xd2, 0x2f,
            0x00, 0x00, 0x00, 0x01, 0xa7, 0xef, 0x6b, 0xf6, 0xff,
        ],
        &[
            0xf7, 0xef, 0x71, 0xf6, 0xa3, 0x10, 0x00, 0x20, 0x00, 0xb8, 0xb0, 0x2d, 0x00, 0x00,
            0x00, 0x7a, 0x7a, 0x00, 0xa6, 0xd2, 0x5c, 0x00, 0x00, 0x00, 0xb1, 0xa8, 0x07, 0xa7,
            0xa2, 0xa6, 0xd2, 0x4d, 0x00, 0x00, 0x00, 0xb1, 0xa8, 0x07, 0xa7, 0x7a, 0x7a, 0x00,
            0x63, 0x64, 0x00, 0x65, 0x66, 0x00, 0xa3, 0xa3, 0xef, 0x71, 0xf6, 0xa1, 0x01, 0xa7,
            0xa3, 0xb5, 0xd2, 0x4e, 0x00, 0x00, 0x00, 0x01, 0xb2, 0xa7, 0xee, 0xa2, 0x61, 0x62,
            0xff,
        ],
        &[
            0xf7, 0xef, 0x6b, 0xf6, 0x10, 0x00, 0x20, 0x00, 0xb8, 0xb5, 0xd2, 0x19, 0x00, 0x00,
            0x00, 0xb1, 0x10, 0x00, 0x20, 0x00, 0xb8, 0xa2, 0xb0, 0x5c, 0x00, 0x00, 0x00, 0xa2,
            0x63, 0x64, 0x00, 0x65, 0x66, 0x00, 0xef, 0x71, 0xf6, 0xa6, 0x01, 0xb2, 0xa8, 0xd2,
            0x28, 0x00, 0x00, 0x00, 0x00, 0xb4, 0xa7, 0xb5, 0xd2, 0x14, 0x00, 0x00, 0x00, 0x01,
            0xa7, 0xee, 0xb5, 0xd2, 0x24, 0x00, 0x00, 0x00, 0x01, 0xa7, 0x00, 0xa1, 0x01, 0xa7,
            0xff,
        ],
        &[
            0xf7, 0xef, 0x71, 0xf6, 0xa3, 0xa1, 0x01, 0xa7, 0x00, 0xb6, 0xa1, 0x01, 0xa7, 0x7a,
            0x7a, 0x00, 0xef, 0x6b, 0xf6, 0xb5, 0xd2, 0x46, 0x00, 0x00, 0x00, 0x01, 0xa7, 0x63,
            0x64, 0x00, 0x65, 0x66, 0x00, 0xb5, 0xd2, 0x62, 0x00, 0x00, 0x00, 0x01, 0xa7, 0xee,
            0xb5, 0xd2, 0x1f, 0x00, 0x00, 0x00, 0x01, 0xb2, 0xa7, 0xa3, 0xa2, 0xb5, 0xd2, 0x31,
            0x00, 0x00, 0x00, 0x01, 0xb2, 0xa7, 0xb5, 0xd2, 0x2a, 0x00, 0x00, 0x00, 0x01, 0xb2,
            0xa7, 0xff,
        ],
    ];
    for (n, body) in residues.iter().enumerate() {
        let s = script(body);
        let first = with_order(&s, ORDERS[0]);
        for order in &ORDERS[1..] {
            assert!(
                with_order(&s, *order) == first,
                "residue {n}, order {order}"
            );
        }
    }
}

#[test]
fn a_searchs_need_runs_on_into_the_next_table() {
    // A direct search of `t` = "ab\0" and a print of `t2` = "cd\0ef\0" right after it; the
    // a1's landing jumps into "ef". The search reads on from `t` into `t2` and through all of
    // it, so `t2` keeps "ef" over that landing's code and the landing stops there; the search
    // reads on past the script's end.
    let t = BODY + 37;
    let (t2, w) = (t + 3, t + 7);
    let mut body = b"\xf7\xef*\xf6".to_vec();
    body.extend(search_of(t, false));
    body.push(0xb5);
    body.extend(lit32(t2));
    body.extend([0xb1, 0xa1, 0x01, 0xa7]);
    body.extend(jump(BODY));
    body.push(0xa2);
    body.extend(jump(w));
    body.extend(b"ab\0cd\0ef\0");
    let s = script(&body);
    let d = any_order(&s);
    let Kind::Table(tb) = &item(&d, t2).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.entries.len(), 2);
    // (The block has no `ee`: its loops find none.)
    assert_eq!(
        sorted_hazards(&d),
        [
            Hazard {
                at: BODY + 1,
                kind: HazardKind::ScanPastEnd(Scan::Answered)
            },
            Hazard {
                at: BODY + 3,
                kind: HazardKind::ScanPastEnd(Scan::NoMatch)
            },
            Hazard {
                at: BODY + 4,
                kind: HazardKind::SearchPastEnd(Span {
                    offset: d.len,
                    len: 0
                })
            },
            Hazard {
                at: w,
                kind: HazardKind::IntoData
            }
        ]
    );
}

#[test]
fn a_string_crossing_a_word_address_holds_no_address_inside_it() {
    // A print of `t` = "ab\0", then "c\xb8de\0"; a search from `t + 4`, the `b8`, and a word
    // write at `t + 5`, both on direct paths. The word address ends the strings `t`'s inner
    // addresses are looked for in: the second string runs past it, so `t + 4` is no address
    // inside `t` but a table of its own, a `b8` alone.
    let t = BODY + 30;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.push(0xb1);
    body.extend(search_of(t + 4, false));
    body.push(0xa6);
    body.extend(lit32(t + 5));
    body.extend([0xb1, 0xa8, 0x07, 0xa7, 0xb6]);
    body.extend(b"ab\0c\xb8de\0");
    let d = any_order(&script(&body));
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(tb.inner.is_empty() && tb.entries.len() == 1);
    let Kind::Table(b8) = &item(&d, t + 4).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(b8.terminated && b8.entries.is_empty() && !b8.words);
}

#[test]
fn a_search_from_a_b8_keeps_only_the_string_it_is_in() {
    // Direct: a print of `t` = "ab\xb8cd\0" and a search from its `b8`; then "ef\0", where
    // the a1's landing jumps and reads "ef". The search reads nothing: the table keeps the
    // string the print reads and the `b8` is in, not "ef", and that landing's code stays.
    let t = BODY + 37;
    let y = t + 6;
    let mut body = b"\xf7\xef*\xf6\xb5".to_vec();
    body.extend(lit32(t));
    body.push(0xb1);
    body.extend(search_of(t + 2, false));
    body.extend([0xa1, 0x01, 0xa7]);
    body.extend(jump(BODY));
    body.push(0xa2);
    body.extend(jump(y));
    body.extend(b"ab\xb8cd\0ef\0");
    let s = script(&body);
    let d = any_order(&s);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.entries.len(), 1);
    assert!(matches!(item(&d, y).kind, Kind::Text(_)), "{:?}", shape(&d));
    assert!(!d.hazards.iter().any(|h| h.kind == HazardKind::IntoData));
}

/// `a6 d2 <w> <i> a8 07 a7` (`i` 0 is `b1`): write word `i` of the table at `w`. 10 bytes.
fn write_word(w: u32, i: u8) -> Vec<u8> {
    [
        &[0xa6][..],
        &lit32(w),
        &[if i == 0 { 0xb1 } else { i }, 0xa8, 0x07, 0xa7],
    ]
    .concat()
}

/// The `*` block's answer (direct code) makes `op(t)` and jumps back; its loops (a landing)
/// make `land(t)` and jump back; then `t` = `tail` to the script's end. Returns the script,
/// `t` and the loops' landing.
fn a_direct_read_and_a_landings(
    op: &dyn Fn(u32) -> Vec<u8>,
    land: &dyn Fn(u32) -> Vec<u8>,
    tail: &[u8],
) -> (Vec<u8>, u32, u32) {
    let at = BODY + 4 + op(0).len() as u32 + 5 + 1;
    let t = at + land(0).len() as u32 + 5;
    let mut b = b"\xf7\xef*\xf6".to_vec();
    b.extend(op(t));
    b.extend(jump(BODY));
    b.push(0xee);
    b.extend(land(t));
    b.extend(jump(BODY));
    b.extend_from_slice(tail);
    (script(&b), t, at)
}

/// The landings `d` dropped, with their reasons.
fn drops(d: &Disasm) -> Vec<Hazard> {
    sorted_hazards(d)
        .into_iter()
        .filter(|h| matches!(h.kind, HazardKind::Dropped(..)))
        .collect()
}

#[test]
fn a_landings_table_inside_a_direct_word_table_drops_the_landing() {
    // The direct write of word 1 of `w` = three words needs two. The loops' string print at
    // `w + 2`, or word write at `w + 1` (inside a word), would cut it: they are dropped and the
    // direct table keeps its words. A word write at `w + 2` (a word's start) chains; aimed at
    // direct code, the loops are dropped as code.
    let words = b"\x10\x00\x20\x00\x30\x00";
    let op = |w: u32| write_word(w, 1);
    let both = |at: u32, reason: Reason| {
        vec![
            dropped(at, Scan::Answered, reason),
            dropped(at, Scan::NoMatch, reason),
        ]
    };
    let print0 = |t: u32| [&[0xb5][..], &lit32(t), &[0xb1]].concat();
    type Land<'a> = &'a dyn Fn(u32) -> Vec<u8>;
    let cases: [(&str, Land, Option<Reason>); 4] = [
        (
            "a string at w + 2",
            &|w| print0(w + 2),
            Some(Reason::TableKinds),
        ),
        (
            "a word at w + 1",
            &|w| write_word(w + 1, 0),
            Some(Reason::TableKinds),
        ),
        ("a word at w + 2", &|w| write_word(w + 2, 0), None),
        ("direct code", &|_| print0(BODY), Some(Reason::TableInCode)),
    ];
    for (what, land, reason) in cases {
        let (s, w, at) = a_direct_read_and_a_landings(&op, land, words);
        let d = any_order(&s);
        assert_eq!(
            drops(&d),
            reason.map_or(Vec::new(), |r| both(at, r)),
            "{what}"
        );
        let Kind::Table(tb) = &item(&d, w).kind else {
            panic!("{what}: {:?}", shape(&d))
        };
        assert!(tb.words, "{what}");
        if reason.is_some() {
            assert_eq!(tb.entries.len(), 3, "{what}");
        }
    }
    // A direct word search of `w` = two words and a `b8`, the loops' string print at `w + 2`.
    let (s, w, at) = a_direct_read_and_a_landings(
        &|w| search_of(w, true),
        &|w| print0(w + 2),
        b"\x10\x00\x20\x00\xb8",
    );
    let d = any_order(&s);
    assert_eq!(drops(&d), both(at, Reason::TableKinds));
    let Kind::Table(tb) = &item(&d, w).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(tb.words && tb.terminated && tb.entries.len() == 2);
}

#[test]
fn a_b8_that_ends_a_direct_table_is_read_only_as_an_end() {
    // A direct string search of `t` = "ab\0" and a `b8`, and the loops' word search of that
    // `b8`: both read it only as an end (the word search reads nothing), so the loops are not
    // dropped. With both searches direct, the same layout decodes with no hazard.
    let (s, t, _) = a_direct_read_and_a_landings(
        &|t| search_of(t, false),
        &|t| search_of(t + 3, true),
        b"ab\0\xb8",
    );
    let d = any_order(&s);
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
    let Kind::Table(tb) = &item(&d, t + 3).kind else {
        panic!("{:?}", shape(&d))
    };
    assert!(tb.words && tb.terminated && tb.entries.is_empty());
    let t = BODY + 25;
    let mut body = search_of(t, false);
    body.extend(search_of(t + 3, true));
    body.push(0xb6);
    body.extend(b"ab\0\xb8");
    let d = any_order(&script(&body));
    assert!(d.hazards.is_empty(), "{:?}", d.hazards);
    // A word search from a `b8` inside a string a direct print reads reads nothing, but its
    // table would hold that byte of the string: the loops are dropped.
    let (s, _, at) = a_direct_read_and_a_landings(
        &|t| [&[0xb5][..], &lit32(t), &[0xb1]].concat(),
        &|t| search_of(t + 2, true),
        b"ab\xb8cd\0",
    );
    assert_eq!(
        drops(&any_order(&s)),
        [
            dropped(at, Scan::Answered, Reason::TableKinds),
            dropped(at, Scan::NoMatch, Reason::TableKinds)
        ]
    );
}

#[test]
fn the_bytes_a_direct_search_reads_on_are_closed_to_landings() {
    // A direct search of `t` = "ab\0" and bytes with no NUL or `b8` to the script's end: it
    // reads on over them. The loops' word table there is dropped; their jump there stops at
    // them; aimed at direct code, they are dropped as code; and alone the search reads on past
    // the script's end.
    let search = |t: u32| search_of(t, false);
    let past = |d: &Disasm, t: u32| Hazard {
        at: BODY + 4,
        kind: HazardKind::SearchPastEnd(Span {
            offset: t + 3,
            len: d.len - (t + 3),
        }),
    };
    let (s, t, at) = a_direct_read_and_a_landings(&search, &|t| write_word(t + 5, 0), b"ab\0cdef");
    let d = any_order(&s);
    assert_eq!(
        sorted_hazards(&d),
        [
            past(&d, t),
            dropped(at, Scan::Answered, Reason::TableKinds),
            dropped(at, Scan::NoMatch, Reason::TableKinds)
        ]
    );
    let (s, t, _) = a_direct_read_and_a_landings(&search, &|t| jump(t + 5), b"ab\0cd\xb6");
    let d = any_order(&s);
    assert_eq!(
        sorted_hazards(&d),
        [
            past(&d, t),
            Hazard {
                at: t + 5,
                kind: HazardKind::IntoData
            }
        ]
    );
    let (s, t, at) = a_direct_read_and_a_landings(&search, &|_| write_word(BODY, 0), b"ab\0cdef");
    let d = any_order(&s);
    assert_eq!(
        sorted_hazards(&d),
        [
            past(&d, t),
            dropped(at, Scan::Answered, Reason::TableInCode),
            dropped(at, Scan::NoMatch, Reason::TableInCode)
        ]
    );
    let t = BODY + 13;
    let mut body = search(t);
    body.push(0xb6);
    body.extend(b"ab\0cdef");
    let d = any_order(&script(&body));
    let alone = Hazard {
        at: BODY,
        ..past(&d, t)
    };
    assert_eq!(d.hazards, [alone]);
}

#[test]
fn a_search_past_its_last_entry_onto_an_item_reads_past_its_table() {
    // "ab\0" at `t`, and right after it the `b6` a jump goes to: with no entry matching, the
    // search reads that code. A direct path's search refuses the script.
    let t = BODY + 17;
    let mut body = search_of(t, false);
    body.extend(jump(t + 3));
    body.extend(b"ab\0\xb6");
    assert_eq!(
        refused(&script(&body)),
        Refusal {
            at: BODY,
            reason: Reason::TableIndex
        }
    );
    // On a landing's path, the landing is dropped.
    let t = BODY + 12 + 17;
    let mut tail = search_of(t, false);
    tail.extend(jump(t + 3));
    tail.extend(b"ab\0\xb6");
    let d = ok(&after_a_block(&tail));
    assert!(both_dropped(&d, Reason::TableIndex), "{:?}", d.hazards);
}

#[test]
fn a_searched_table_wins_over_another_landings_code_up_to_its_b8() {
    // The a1's landing runs "cd" at `y`; the block's loops search `t` = "ab\0", then "cd\0"
    // and the script's end. The search reads "cd" as its second entry: the table keeps it and
    // the a1's landing stops there; the search reads on past the end.
    let x = BODY + 13;
    let t = x + 23;
    let y = t + 3;
    let mut rest = jump(y);
    rest.push(0xee);
    rest.extend(search_of(t, false));
    rest.extend(jump(BODY));
    rest.extend(b"ab\0cd\0");
    let s = after_an_if(&rest);
    let [a, b] = both_orders(&s);
    assert!(a == b, "{:?}\n{:?}", a.1, b.1);
    let d = ok(&s);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(
        tb.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [&b"ab"[..], b"cd"]
    );
    assert_eq!(
        sorted_hazards(&d),
        [
            Hazard {
                at: x + 6,
                kind: HazardKind::SearchPastEnd(Span {
                    offset: t + 6,
                    len: 0
                })
            },
            Hazard {
                at: y,
                kind: HazardKind::IntoData
            }
        ]
    );
}

#[test]
fn skip_loops_step_over_each_operand_width() {
    // A false `a1` steps over `b0`'s 4 operand bytes, `d3`'s 1 and `d4`'s 2: a stop byte in
    // the last one stepped over is not a stop. The interpreter lands where the decoder does.
    let j = BODY + 9;
    let cases: [(Vec<u8>, u16, &[&str]); 3] = [
        (
            vec![
                0xa1, 0x00, 0xa7, 0xb0, j as u8, 0x00, 0x00, 0xa2, 0xa2, 0xb6,
            ],
            9,
            &["if", "jump", "endif", "bye"],
        ),
        (
            vec![0xa1, 0x00, 0xa7, 0xc4, 0xd3, 0xa2, 0xa7, 0xa2, 0xb6],
            8,
            &["if", "add_karma", "endif", "bye"],
        ),
        (
            vec![0xa1, 0x00, 0xa7, 0xc4, 0xd4, 0x00, 0xa2, 0xa7, 0xa2, 0xb6],
            9,
            &["if", "add_karma", "endif", "bye"],
        ),
    ];
    for (body, after, want) in cases {
        let d = ok(&script(&body));
        assert_eq!(&shape(&d)[5..], want, "{body:02x?}");
        assert!(d.hazards.is_empty(), "{body:02x?}: {:?}", d.hazards);
        let (_, s) = exec_on(&body, "");
        assert_eq!(s.converse.pc, after, "{body:02x?}");
    }
}

#[test]
fn the_evaluators_stack_bounds_are_hazards_and_a_table_beyond_them_is_none() {
    // `81` reads below the stack; the nullary `c0` after it does not.
    let d = ok(&script(&[0xc4, 0x81, 0xc0, 0xa7, 0xb6]));
    assert_eq!(
        d.hazards,
        [Hazard {
            at: BODY + 1,
            kind: HazardKind::Underflow
        }]
    );
    // Ten values fill the stack; the table address above them is dropped (no table).
    let t = BODY + 30;
    let mut body = vec![0xa6, 0x01, 0xb2, 0xa8];
    body.extend(1..=10u8);
    body.extend(lit32(t));
    body.extend([0x01, 0xb4, 0xa7, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend([0x10, 0x00, 0xb8]);
    let d = ok(&script(&body));
    assert!(
        d.hazards.contains(&Hazard {
            at: BODY + 14,
            kind: HazardKind::Overflow
        }),
        "{:?}",
        d.hazards
    );
    assert!(!d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))));
}

#[test]
fn an_address_inside_another_tables_string_reads_that_strings_rest() {
    // Prints of the string at `t`, of its rest from `t + 1` and of the string after that rest
    // (index 1 from `t + 1`); a copy of index 1 from `t + 1` marks that string. One table.
    let t = BODY + 40;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.push(0xb1);
    body.push(0xb5);
    body.extend(lit32(t + 1));
    body.push(0xb1);
    body.push(0xb5);
    body.extend(lit32(t + 1));
    body.extend([0x01, 0xa7]);
    body.extend([0xa6, 0x03, 0xb3, 0xa8]);
    body.extend(lit32(t + 1));
    body.extend([0x01, 0xa7, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"ab\0cd\0\xb8");
    let s = script(&body);
    let d = ok(&s);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.inner, [t + 1]);
    assert_eq!(
        tb.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [&b"ab"[..], b"cd"]
    );
    let units = ScriptUnits::units(&d, &s);
    let slots: Vec<&[u8]> = units.strings.iter().map(|u| &u.slots[..]).collect();
    assert_eq!(slots, [&[][..], &[3]]);
    // Index 2 from `t + 1` is past the table.
    let mut far = body.clone();
    let at = far.windows(2).position(|w| w == [0x01, 0xa7]).unwrap();
    far[at] = 0x02;
    assert_eq!(refused(&script(&far)).reason, Reason::TableIndex);
    // The cross-check takes a read from `t + 1` to the string's NUL, not from any other byte.
    let read = |from: u32| {
        let ev: Vec<Event> = (from..t + 3)
            .map(|at| Event::Table {
                at: at as i32,
                byte: s[at as usize],
                read: Read::Copy,
            })
            .collect();
        checked(&body, ev)
    };
    assert_eq!(read(t + 1).failures(), 0);
    assert_eq!(read(t + 2).table_mismatches, 1);
}

#[test]
fn several_addresses_inside_one_string_are_all_of_its_table() {
    // Prints of the string at `t` and of its rest from `t + 1`, `t + 2` and `t + 3`, all
    // before its NUL; then index 1 from `t + 3`, the string after it.
    let t = BODY + 40;
    let mut body = Vec::new();
    for at in [t, t + 1, t + 2, t + 3] {
        body.push(0xb5);
        body.extend(lit32(at));
        body.push(0xb1);
    }
    body.push(0xb5);
    body.extend(lit32(t + 3));
    body.extend([0x01, 0xa7, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"abcd\0ef\0\xb8");
    let s = script(&body);
    let d = ok(&s);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.inner, [t + 1, t + 2, t + 3]);
    assert_eq!(
        tb.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [&b"abcd"[..], b"ef"]
    );
}

#[test]
fn an_address_at_a_strings_nul_reads_its_empty_rest() {
    // Prints of the string at `t`, from the NUL that ends it (an empty rest) and of index 1
    // from there, the string after it. One table; a read at the NUL is an empty string.
    let t = BODY + 30;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.extend([0xb1, 0xb5]);
    body.extend(lit32(t + 2));
    body.extend([0xb1, 0xb5]);
    body.extend(lit32(t + 2));
    body.extend([0x01, 0xa7, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"ab\0cd\0\xb8");
    let s = script(&body);
    let d = ok(&s);
    let Kind::Table(tb) = &item(&d, t).kind else {
        panic!("{:?}", shape(&d))
    };
    assert_eq!(tb.inner, [t + 2]);
    assert_eq!(
        tb.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
        [&b"ab"[..], b"cd"]
    );
    let nul = Event::Table {
        at: (t + 2) as i32,
        byte: 0,
        read: Read::Copy,
    };
    let n = checked(&body, vec![nul]);
    assert_eq!((n.table_strings, n.failures()), (1, 0));
}

/// After [`after_an_if`]'s `a1`, whose landing jumps to `to` and runs it as text: the `*`
/// block's `ee`, then its loops print string 0 of `t` = "ab\0cd\0" (no `b8`), read at `u` =
/// `t + 4` (inside "cd") with `op` and jump back. `to` is `u`, or `t + 3` (the start of "cd")
/// when `at_string`. Returns the script, `t`, `u` and `to`.
fn an_inner_read_over_an_ifs_code(op: fn(u32) -> Vec<u8>, at_string: bool) -> (Vec<u8>, [u32; 3]) {
    // jump (5), ee, b5 d2 t b1 (7), op, jump (5).
    let t = BODY + 13 + 5 + 1 + 7 + op(0).len() as u32 + 5;
    let u = t + 4;
    let to = if at_string { t + 3 } else { u };
    let mut rest = jump(to);
    rest.extend([0xee, 0xb5]);
    rest.extend(lit32(t));
    rest.push(0xb1);
    rest.extend(op(u));
    rest.extend(jump(BODY));
    rest.extend(b"ab\0cd\0");
    (after_an_if(&rest), [t, u, to])
}

/// `b5 d2 u 01 b2 a7`: a print from `u` with a computed index.
fn print_computed(u: u32) -> Vec<u8> {
    [&[0xb5][..], &lit32(u), &[0x01, 0xb2, 0xa7]].concat()
}

/// `a6 01 b2 a8 d2 u 03 b7 a7`: a word variable set to a search of `u` for slot 3.
fn search_from(u: u32) -> Vec<u8> {
    [
        &[0xa6, 0x01, 0xb2, 0xa8][..],
        &lit32(u),
        &[0x03, 0xb7, 0xa7],
    ]
    .concat()
}

#[test]
fn an_inner_address_read_with_no_constant_index_keeps_its_string_in_either_order() {
    // The read at `u` needs "cd", where the a1's landing runs: the table keeps it and wins, and
    // the a1's landing stops there, whichever landing comes first.
    for (what, op) in [
        ("print", print_computed as fn(u32) -> Vec<u8>),
        ("search", search_from),
    ] {
        for at_string in [false, true] {
            let (s, [t, u, to]) = an_inner_read_over_an_ifs_code(op, at_string);
            let what = format!("{what}, landing at the string's start {at_string}");
            let [a, b] = both_orders(&s);
            assert!(a == b, "{what}: {:?}\n{:?}", a.1, b.1);
            let d = ok(&s);
            let Kind::Table(tb) = &item(&d, t).kind else {
                panic!("{what}: {:?}", shape(&d))
            };
            assert_eq!(
                tb.entries.iter().map(|e| e.bytes(&s)).collect::<Vec<_>>(),
                [&b"ab"[..], b"cd"],
                "{what}"
            );
            assert_eq!(tb.inner, [u], "{what}");
            let mut want = vec![Hazard {
                at: to,
                kind: HazardKind::IntoData,
            }];
            // With no `b8`, a search that matches nothing reads on past the script's end.
            if what.starts_with("search") {
                want.push(Hazard {
                    at: t - 5 - op(0).len() as u32,
                    kind: HazardKind::SearchPastEnd(Span {
                        offset: t + 6,
                        len: 0,
                    }),
                });
            }
            assert_eq!(d.hazards, want, "{what}");
        }
    }
}

#[test]
fn indices_and_addresses_have_the_interpreters_width() {
    let t = BODY + 20;
    let strings = |op: &[u8]| {
        let mut body = op.to_vec();
        body.resize((t - BODY) as usize, 0);
        body.extend(b"a\0b\0");
        script(&body)
    };
    let words = |op: &[u8]| {
        let mut body = op.to_vec();
        body.resize((t - BODY) as usize, 0);
        body.extend([0x10, 0x00, 0x20, 0x00, 0xb8]);
        script(&body)
    };
    // A string index is a word: -1 reads the first string, 0x10001 the second (its copy marks
    // that string, and with one string only it is past the table).
    let copy = |index: u32| {
        [
            &[0xa6, 0x03, 0xb3, 0xa8][..],
            &lit32(t),
            &lit32(index),
            &[0xa7, 0xb6],
        ]
        .concat()
    };
    for (index, picked) in [(0xffff_ffffu32, 0), (0x0001_0001, 1)] {
        let s = strings(&copy(index));
        let units = ScriptUnits::units(&ok(&s), &s);
        let slots: Vec<&[u8]> = units.strings.iter().map(|u| &u.slots[..]).collect();
        let mut want: Vec<&[u8]> = vec![&[], &[]];
        want[picked] = &[3];
        assert_eq!(slots, want, "index {index:#x}");
    }
    let one = |op: &[u8]| {
        let mut body = op.to_vec();
        body.resize((t - BODY) as usize, 0);
        body.extend(b"a\0");
        script(&body)
    };
    assert_eq!(refused(&one(&copy(0x0001_0001))).reason, Reason::TableIndex);
    ok(&one(&copy(0xffff_ffff)));
    // A word index moves by twice itself in word arithmetic: 0x10001 reads word 1, -1 no word.
    let w_op = |index: u32| {
        [
            &[0xa6][..],
            &lit32(t),
            &lit32(index),
            &[0xa8, 0x07, 0xa7, 0xb6],
        ]
        .concat()
    };
    let w = |index: u32| words(&w_op(index));
    let d = ok(&w(0x0001_0001));
    let Operands::Assign(Assign::TableWord { table, .. }) = &op_at(&d, BODY).operands else {
        panic!()
    };
    assert_eq!(ref_entry(table, true), Some(1));
    assert_eq!(refused(&w(0xffff)).reason, Reason::TableIndex);
    // 0x8001 is -32767: twice it wraps to 2 in word arithmetic, word 1.
    let d = ok(&w(0x8001));
    let Operands::Assign(Assign::TableWord { table, .. }) = &op_at(&d, BODY).operands else {
        panic!()
    };
    assert_eq!(ref_entry(table, true), Some(1));
    // `b4`'s index is its low byte: 0x101 reads word 1; with one word that is past the table.
    let b4 = [
        &[0xa6, 0x01, 0xb2, 0xa8][..],
        &lit32(t),
        &[0xd4, 0x01, 0x01, 0xb4, 0xa7, 0xb6],
    ]
    .concat();
    ok(&words(&b4));
    let one_word = |op: &[u8]| {
        let mut body = op.to_vec();
        body.resize((t - BODY) as usize, 0);
        body.extend([0x10, 0x00, 0xb8]);
        script(&body)
    };
    assert_eq!(refused(&one_word(&b4)).reason, Reason::TableIndex);
    for index in [0x0001_0001, 0x8001] {
        assert_eq!(refused(&one_word(&w_op(index))).reason, Reason::TableIndex);
    }
    // An address is the low word of its operand.
    let d = ok(&strings(
        &[&[0xb5][..], &lit32(0x0001_0000 + t), &[0xb1, 0xb6]].concat(),
    ));
    assert!(
        matches!(item(&d, t).kind, Kind::Table(_)),
        "{:?}",
        shape(&d)
    );
}

#[test]
fn a_bye_in_the_look_text_goes_on_to_the_greeting_after_it() {
    // The look text jumps past the first `f2` to a `b6`: `run_text` returns there and the scan
    // for `f2` starts at the `b6`, so the second greeting is read.
    let x = 16u32;
    let mut s = b"\xff\x05Nm\xf1lk".to_vec();
    s.extend(jump(x));
    s.extend(b"\xf2Hi\xb6\xb6z\xf2Yo\xb6");
    let d = ok(&s);
    assert_eq!(
        shape(&d),
        [
            "hdr",
            "name",
            "f1",
            "text2",
            "jump",
            "f2",
            "text2",
            "bye",
            "bye",
            "unreached1",
            "f2",
            "text2",
            "bye"
        ]
    );
}

#[test]
fn a_script_longer_than_the_script_buffer_is_refused() {
    let mut s = script(&[0xb6]);
    s.resize(crate::converse::SCRIPT_LEN, 0);
    ok(&s);
    s.push(0);
    assert_eq!(
        refused(&s),
        Refusal {
            at: crate::converse::SCRIPT_LEN as u32,
            reason: Reason::TooLong
        }
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
        let (len, decoded) = Decoder::new(&b, &Plan::default())
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
        let (len, _) = Decoder::new(&s, &Plan::default()).op(0, s[0]).unwrap();
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
        let e = Decoder::new(&b, &Plan::default()).expr(0).unwrap();
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
    s.extend(b"\n\xf7\xefname\xf6I am^ Ann.");
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
        n.offsets > 100 && n.filled >= 6 && n.table_strings == 2 && n.prints == 1,
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
    // The vectors u6-hdpack pins for its content hash, and the FNV reference's "foobar".
    assert_eq!(hash64(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(hash64(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(hash64(b"foobar"), 0x8594_4171_f739_67e8);
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
    let other = json.replace(FORMAT, "u6-converse-catalog/1");
    assert!(
        Catalog::from_json(&other).is_err(),
        "the first format is not this one"
    );
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

#[test]
fn a_copy_reaches_the_tables_that_follow_its_table() {
    // String 1 of `t` runs on into the table a print names at `t + 6`: it keeps its slot and
    // capacity there. A computed index marks every string it can reach.
    let t = BODY + 40;
    let mut body = Vec::new();
    body.extend([0xa6, 0x03, 0xb3, 0xa8]);
    body.extend(lit32(t));
    body.extend([0x01, 0xa7]);
    body.extend([0xa6, 0x04, 0xb3, 0xa8]);
    body.extend(lit32(t));
    body.extend([0x05, 0xb2, 0xa7]);
    body.push(0xb5);
    body.extend(lit32(t + 6));
    body.extend([0xb1, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"first\0second\0");
    let s = script(&body);
    let units = ScriptUnits::units(&ok(&s), &s);
    let strings: Vec<_> = units
        .strings
        .iter()
        .map(|u| {
            (
                u.unit.text.as_str(),
                u.table,
                u.index,
                &u.slots[..],
                u.capacity,
            )
        })
        .collect();
    let cap = Some(SLOT_BUFFER_CAPACITY);
    assert_eq!(
        strings,
        [
            ("first", t, 0, &[4][..], cap),
            ("second", t + 6, 0, &[3, 4][..], cap)
        ]
    );
}

#[test]
fn the_parts_of_a_split_run_are_linked() {
    // The block jumps into the middle of "Hi there": two runs, one sentence.
    let mut body = b"Hi there\xf7\xef*\xf6".to_vec();
    body.extend(jump(BODY + 3));
    body.extend([0xee, 0xff]);
    let s = script(&body);
    let d = ok(&s);
    assert_eq!(d.run_splits, [BODY + 3]);
    let units = ScriptUnits::units(&d, &s);
    let runs: Vec<(&str, Option<u32>)> = units
        .runs
        .iter()
        .map(|r| (r.unit.text.as_str(), r.part_of))
        .collect();
    assert_eq!(
        runs,
        [("look", None), ("Hi ", Some(BODY)), ("there", Some(BODY))]
    );
    // Runs that only touch across an op are not one run.
    let s = script(b"Hello \xa7world.\xb6");
    let units = ScriptUnits::units(&ok(&s), &s);
    let runs: Vec<(&str, Option<u32>)> = units
        .runs
        .iter()
        .map(|r| (r.unit.text.as_str(), r.part_of))
        .collect();
    assert_eq!(runs, [("look", None), ("Hello ", None), ("world.", None)]);
}

#[test]
fn a_catalog_reads_back_only_with_its_split_links_as_written() {
    // Two split runs: the first block jumps into "Hi there", the second into "Yo dude".
    let mut body = b"Hi there\xf7\xefa\xf6".to_vec();
    body.extend(jump(BODY + 3));
    body.extend(b"\xee\xefb\xf6Yo dude");
    let dude = BODY + body.len() as u32 - 4;
    body.extend(jump(dude));
    body.extend([0xee, 0xff]);
    let s = script(&body);
    let c = Catalog::build([(b'a', 7, s.as_slice())]);
    let links: Vec<(&str, Option<u32>)> = c.scripts[0]
        .runs
        .iter()
        .map(|r| (r.unit.text.as_str(), r.part_of))
        .collect();
    let yo = dude - 3;
    assert_eq!(
        links,
        [
            ("look", None),
            ("Hi ", Some(BODY)),
            ("there", Some(BODY)),
            ("Yo ", Some(yo)),
            ("dude", Some(yo)),
        ]
    );
    assert!(Catalog::from_json(&c.to_json()).is_ok());
    let tampered = |i: usize, link: Option<u32>| {
        let mut t = c.clone();
        t.scripts[0].runs[i].part_of = link;
        Catalog::from_json(&t.to_json())
    };
    for (i, link, what) in [
        (2, Some(BODY + 1), "a link to no run"),
        (
            1,
            Some(BODY + 3),
            "a first part linked to a later one (a cycle)",
        ),
        (3, Some(BODY), "a part of one group linked to another"),
        (4, Some(BODY), "a part linked to a group it does not follow"),
        (1, None, "a group whose first part links to nothing"),
        (2, None, "a first part alone"),
        (
            0,
            Some(5),
            "a run outside a group linked to its own start, alone",
        ),
        (0, Some(BODY), "a run linked to a group after it"),
    ] {
        assert!(tampered(i, link).is_err(), "{what}");
    }
    let mut swapped = c.clone();
    swapped.scripts[0].runs.swap(1, 2);
    assert!(
        Catalog::from_json(&swapped.to_json()).is_err(),
        "runs out of order"
    );
}

#[test]
fn a_catalog_reads_back_only_as_written() {
    let s = conversation();
    let c = Catalog::build([(b'a', 3, s.as_slice())]);
    let json = c.to_json();
    assert_eq!(Catalog::from_json(&json).unwrap(), c);
    let h = format!("{:016x}", c.scripts[0].hash);
    for bad in [
        h.to_uppercase(),
        format!("+{}", &h[1..]),
        format!("{}x", &h[1..]),
    ] {
        assert!(
            Catalog::from_json(&json.replace(&h, &bad)).is_err(),
            "{bad:?}"
        );
    }
    // A unit whose text, length, place or hash disagree.
    let unit = &c.scripts[0].runs[1].unit;
    let at = format!("\"offset\":{},\"len\":{}", unit.offset, unit.len);
    for bad in [
        format!("\"offset\":{},\"len\":{}", unit.offset, unit.len + 1),
        format!("\"offset\":{},\"len\":{}", s.len(), unit.len),
    ] {
        assert!(
            Catalog::from_json(&json.replacen(&at, &bad, 1)).is_err(),
            "{bad}"
        );
    }
    let text = format!("\"text\":\"{}\"", unit.text);
    let other = format!("\"text\":\"{}\"", unit.text.replace('e', "E"));
    assert_ne!(text, other);
    assert!(Catalog::from_json(&json.replacen(&text, &other, 1)).is_err());
}

#[test]
fn a_catalog_string_lists_the_addresses_read_inside_it() {
    // Prints from `t`, `t + 1` and `t + 3` (inside "abcd") and from `t + 6` (inside "ef").
    let t = BODY + 40;
    let mut body = Vec::new();
    for at in [t, t + 1, t + 3, t + 6] {
        body.push(0xb5);
        body.extend(lit32(at));
        body.push(0xb1);
    }
    body.push(0xb6);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"abcd\0ef\0\xb8");
    let s = script(&body);
    let c = Catalog::build([(b'a', 7, s.as_slice())]);
    let inner: Vec<&[u32]> = c.scripts[0].strings.iter().map(|u| &u.inner[..]).collect();
    assert_eq!(inner, [&[t + 1, t + 3][..], &[t + 6]]);
    let json = c.to_json();
    assert_eq!(Catalog::from_json(&json).unwrap(), c);
    // An address outside its string (in the one before, at its start, past its NUL), or out
    // of order, does not read back.
    let written = format!("\"inner\":[{}]", t + 6);
    assert!(json.contains(&written));
    for bad in [t + 2, t + 5, t + 8] {
        let other = json.replace(&written, &format!("\"inner\":[{bad}]"));
        assert!(Catalog::from_json(&other).is_err(), "{bad}");
    }
    let other = json.replace(
        &format!("\"inner\":[{},{}]", t + 1, t + 3),
        &format!("\"inner\":[{},{}]", t + 3, t + 1),
    );
    assert_ne!(other, json);
    assert!(Catalog::from_json(&other).is_err());
}

// ---- the cross-check's own checks ------------------------------------------------------------

fn esc() -> EscState {
    EscState {
        prefix: 0,
        hidden: false,
        plural: false,
        runic: false,
        show_hidden: false,
        keyword_end: Vec::new(),
    }
}

fn first_snap() -> Option<Box<Snap>> {
    Some(Box::new(Snap {
        esc: esc(),
        strs: Vec::new(),
        ints: Vec::new(),
    }))
}

/// The cross-check of a script whose body is `body`, on `events` after its load.
fn checked(body: &[u8], events: Vec<Event>) -> trace::Counts {
    let s = script(body);
    let d = ok(&s);
    let scripts: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 7),
        Decoded {
            bytes: &s,
            disasm: &d,
        },
    )]
    .into();
    let mut ev = vec![Event::Load {
        letter: b'a',
        item: 7,
    }];
    ev.extend(events);
    trace::check(&ev, &scripts)
}

/// The reads of a visit of `text` at `BODY` and its stop byte `b6`, with `out` put out at the
/// visit's last byte and `before` before it.
fn visit(text: &[u8], before: &str, out: &str) -> Vec<Event> {
    let emit = |s: &str| s.bytes().map(|b| put(i16::from(b))).collect::<Vec<_>>();
    visit_events(text, emit(before), emit(out))
}

/// The escape layer put `c`.
fn put(c: i16) -> Event {
    Event::Emit { c, wait: false }
}

/// [`visit`] with the events put before and at the visit's last byte.
fn visit_events(text: &[u8], before: Vec<Event>, out: Vec<Event>) -> Vec<Event> {
    let mut ev = before;
    for (i, &b) in text.iter().enumerate() {
        ev.push(Event::Run {
            at: (BODY as usize + i) as i32,
            byte: b,
            snap: if i == 0 { first_snap() } else { None },
        });
    }
    ev.extend(out);
    ev.push(Event::Run {
        at: (BODY as usize + text.len()) as i32,
        byte: 0xb6,
        snap: None,
    });
    ev
}

#[test]
fn a_visit_must_put_out_its_run_itself_spaces_and_newlines_included() {
    let fills = |text: &[u8], before: &str, out: &str| {
        let body = [text, &[0xb6]].concat();
        let n = checked(&body, visit(text, before, out));
        assert_eq!((n.filled, n.failures() - n.fill_mismatches), (1, 0));
        n.fill_mismatches
    };
    assert_eq!(fills(b"abc", "xy", "abc"), 0);
    // Nothing put out, whatever the scroll already shows.
    assert_eq!(fills(b"abc", "xy", ""), 1);
    assert_eq!(fills(b"abc", "abc", ""), 1);
    assert_eq!(fills(b"abc", "abcabc", ""), 1);
    // Before word wrap: a space and a newline are characters like any other.
    assert_eq!(fills(b"a b", "", "a b"), 0);
    assert_eq!(fills(b"a b", "", "ab"), 1);
    assert_eq!(fills(b"a\nb", "", "a\nb"), 0);
    assert_eq!(fills(b"a\nb", "", "ab"), 1);
}

#[test]
fn a_glyph_of_code_0_is_checked_and_a_key_waits_own_output_is_not_the_runs() {
    // `^` and a space put the glyph of code 0 (`@` and a back-quote would be prefixes of their
    // own); a key wait's flush and the newline its cursor ends a full line with are not the
    // run's.
    let text = b"a^ b";
    let body = [&text[..], &[0xb6]].concat();
    let wait = |c: i16| Event::Emit { c, wait: true };
    let (a, b) = (put(0x61), put(0x62));
    let fills = |out: Vec<Event>| {
        let n = checked(&body, visit_events(text, Vec::new(), out));
        assert_eq!((n.filled, n.failures() - n.fill_mismatches), (1, 0));
        n.fill_mismatches
    };
    assert_eq!(fills(vec![a.clone(), put(0), b.clone()]), 0);
    assert_eq!(fills(vec![a.clone(), b.clone()]), 1, "the glyph is missing");
    assert_eq!(
        fills(vec![a.clone(), wait(0), b.clone()]),
        1,
        "a key wait's 0 is not the glyph"
    );
    assert_eq!(fills(vec![a, put(0), wait(-1), wait(0), b]), 0);
}

#[test]
fn a_table_read_of_one_nul_is_an_empty_string_or_a_failure() {
    // Strings "a" and "" at `t`.
    let t = BODY + 20;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.extend([0x01, 0xa7, b'H', b'i', 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"a\0\0");
    let read = |at: u32| {
        checked(
            &body,
            vec![Event::Table {
                at: at as i32,
                byte: 0,
                read: Read::Copy,
            }],
        )
    };
    let n = read(t + 2);
    assert_eq!(
        (n.failures(), n.table_strings, n.offsets),
        (0, 1, 1),
        "{n:?}"
    );
    // At a string's first byte (not 0, not an empty string); inside a run.
    let n = read(t);
    assert_eq!((n.byte_mismatches, n.table_mismatches), (1, 1), "{n:?}");
    let n = read(BODY + 8);
    assert_eq!((n.byte_mismatches, n.table_mismatches), (1, 1), "{n:?}");
    // A 0 that is right but no empty string: the NUL that ends "a", the padding before the
    // table.
    for at in [t + 1, BODY + 12] {
        let n = read(at);
        assert_eq!(
            (n.byte_mismatches, n.table_mismatches, n.failures()),
            (0, 1, 1),
            "{n:?}"
        );
    }
}

#[test]
fn a_table_print_must_put_out_the_string_it_read() {
    // `b5 d2 t b1` prints "ab" at `t`: the same reads with the string put out, nothing put
    // out, or another character put out.
    let t = BODY + 20;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.extend([0xb1, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"ab\0");
    let prints = |out: &[u8]| {
        let mut ev = vec![
            Event::Run {
                at: BODY as i32,
                byte: 0xb5,
                snap: None,
            },
            Event::PrintStart { snap: first_snap() },
        ];
        for (i, &b) in b"ab\0".iter().enumerate() {
            ev.push(Event::Table {
                at: (t as usize + i) as i32,
                byte: b,
                read: Read::Print,
            });
            if let Some(&c) = out.get(i).filter(|_| b != 0) {
                ev.push(put(i16::from(c)));
            }
        }
        let n = checked(&body, ev);
        assert_eq!(
            (n.prints, n.table_strings, n.failures() - n.print_mismatches),
            (1, 1, 0)
        );
        n.print_mismatches
    };
    assert_eq!(prints(b"ab"), 0);
    assert_eq!(prints(b""), 1, "nothing put out");
    assert_eq!(prints(b"ax"), 1, "another character");
    // A copy or a search puts nothing out: its reads are checked, not an output.
    for read in [Read::Copy, Read::Search] {
        let reads: Vec<Event> = b"ab\0"
            .iter()
            .enumerate()
            .map(|(i, &b)| Event::Table {
                at: (t as usize + i) as i32,
                byte: b,
                read,
            })
            .collect();
        let n = checked(&body, reads);
        assert_eq!(
            (n.prints, n.table_strings, n.failures()),
            (0, 1, 0),
            "{read:?}"
        );
    }
}

#[test]
fn a_string_read_has_one_reader_and_a_print_only_its_own_output() {
    // "ab" at `t`: read for a copy up to its NUL, read there for a print: a mismatch.
    let t = BODY + 20;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.extend([0xb1, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"ab\0");
    let table = |i: usize, read: Read| Event::Table {
        at: (t as usize + i) as i32,
        byte: b"ab\0"[i],
        read,
    };
    let reads = vec![
        table(0, Read::Copy),
        table(1, Read::Copy),
        table(2, Read::Print),
    ];
    let n = checked(&body, reads);
    assert_eq!((n.table_strings, n.table_mismatches), (1, 1), "{n:?}");
    // A print started, put out "x" and ended at another read (the op at BODY): a print of the
    // string after that, with no start of its own, is not compared with that output.
    let mut ev = vec![
        Event::PrintStart { snap: first_snap() },
        put(i16::from(b'x')),
        Event::Run {
            at: BODY as i32,
            byte: 0xb5,
            snap: None,
        },
    ];
    for i in 0..3 {
        ev.push(table(i, Read::Print));
        ev.extend(b"ab".get(i).map(|&c| put(i16::from(c))));
    }
    let n = checked(&body, ev);
    assert_eq!(
        (n.prints, n.table_strings, n.failures()),
        (0, 1, 0),
        "{n:?}"
    );
}

#[test]
fn a_stopped_visit_cut_inside_an_escape_puts_out_nothing_for_it_yet() {
    // "a$1b" read through its `$`: the escape takes the next byte, not read yet, so "a" is all
    // the bytes read put out.
    let text = b"a$1b";
    let body = [&text[..], &[0xb6]].concat();
    let s = script(&body);
    let d = ok(&s);
    let scripts: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 7),
        Decoded {
            bytes: &s,
            disasm: &d,
        },
    )]
    .into();
    let stopped = |out: &[u8]| {
        let mut ev = vec![Event::Load {
            letter: b'a',
            item: 7,
        }];
        for (i, &b) in text[..2].iter().enumerate() {
            ev.push(Event::Run {
                at: (BODY as usize + i) as i32,
                byte: b,
                snap: if i == 0 { first_snap() } else { None },
            });
        }
        ev.extend(out.iter().map(|&c| put(i16::from(c))));
        trace::check_interrupted(&ev, &scripts)
    };
    assert_eq!(stopped(b"a").failures(), 0);
    assert_eq!(stopped(b"a1").fill_mismatches, 1);
}

#[test]
fn a_table_print_stopped_while_it_waits_is_checked_as_far_as_it_read() {
    // `b5 d2 t b1` over "abc" at `t`, stopped after reading "ab".
    let t = BODY + 20;
    let mut body = vec![0xb5];
    body.extend(lit32(t));
    body.extend([0xb1, 0xb6]);
    body.resize((t - BODY) as usize, 0);
    body.extend(b"abc\0");
    let s = script(&body);
    let d = ok(&s);
    let scripts: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 7),
        Decoded {
            bytes: &s,
            disasm: &d,
        },
    )]
    .into();
    let stopped = |out: &[u8]| {
        let mut ev = vec![
            Event::Load {
                letter: b'a',
                item: 7,
            },
            Event::Run {
                at: BODY as i32,
                byte: 0xb5,
                snap: None,
            },
            Event::PrintStart { snap: first_snap() },
        ];
        for (i, &b) in b"ab".iter().enumerate() {
            ev.push(Event::Table {
                at: (t as usize + i) as i32,
                byte: b,
                read: Read::Print,
            });
            ev.extend(out.get(i).map(|&c| put(i16::from(c))));
        }
        // The rest of the output came after the last read.
        ev.extend(out.iter().skip(2).map(|&c| put(i16::from(c))));
        trace::check_interrupted(&ev, &scripts)
    };
    for out in [&b"ab"[..], b"a"] {
        let n = stopped(out);
        assert_eq!((n.prints, n.failures()), (1, 0), "{n:?}");
    }
    // Nothing, output of a byte not read yet, another character.
    for out in [&b""[..], b"abc", b"ax"] {
        let n = stopped(out);
        assert_eq!((n.print_mismatches, n.failures()), (1, 1), "{n:?}");
    }
}

#[test]
fn a_conversation_stopped_while_it_waits_is_checked_as_far_as_it_went() {
    // A visit of "abc" stopped after its second byte: right as far as it went, then a wrong
    // byte after the last input-loop read, then a read outside any run.
    let text = b"abc";
    let body = [&text[..], &[0xb6]].concat();
    let start = |bytes: &[(u32, u8)], out: &[u8]| {
        let mut ev = vec![Event::Input {
            at: BODY as i32 - 1,
            byte: 0xf2,
        }];
        for (i, &(at, b)) in bytes.iter().enumerate() {
            ev.push(Event::Run {
                at: at as i32,
                byte: b,
                snap: if i == 0 { first_snap() } else { None },
            });
        }
        ev.extend(out.iter().map(|&c| put(i16::from(c))));
        ev
    };
    let interrupted = |ev: Vec<Event>| {
        let s = script(&body);
        let d = ok(&s);
        let scripts: BTreeMap<(u8, u16), Decoded<'_>> = [(
            (b'a', 7),
            Decoded {
                bytes: &s,
                disasm: &d,
            },
        )]
        .into();
        let mut all = vec![Event::Load {
            letter: b'a',
            item: 7,
        }];
        all.extend(ev);
        trace::check_interrupted(&all, &scripts)
    };
    let (a, b) = ((BODY, b'a'), (BODY + 1, b'b'));
    // The output so far: all of the first byte's, the second byte's maybe not yet.
    for out in [&b"ab"[..], b"a"] {
        let n = interrupted(start(&[a, b], out));
        assert_eq!((n.failures(), n.filled), (0, 1), "{n:?}");
    }
    let n = interrupted(start(&[a, b], b""));
    assert_eq!(n.fill_mismatches, 1, "the first byte's output is missing");
    // Output of a byte not read yet, or another character, with the reads right.
    for out in [&b"abc"[..], b"ax"] {
        let n = interrupted(start(&[a, b], out));
        assert_eq!((n.fill_mismatches, n.failures()), (1, 1), "{n:?}");
    }
    let n = interrupted(start(&[a, (BODY + 1, b'x')], b"ax"));
    assert!(n.byte_mismatches == 1 && n.failures() >= 1, "{n:?}");
    let n = interrupted(start(&[(BODY + 1, b'b')], b"b"));
    assert_eq!(n.partial_visits, 1, "a visit must start a run");
    // Ended, the same open visit is partial.
    let s = script(&body);
    let d = ok(&s);
    let scripts: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 7),
        Decoded {
            bytes: &s,
            disasm: &d,
        },
    )]
    .into();
    let mut all = vec![Event::Load {
        letter: b'a',
        item: 7,
    }];
    all.extend(start(&[a, b], b"ab"));
    assert_eq!(trace::check(&all, &scripts).partial_visits, 1);
}

/// A conversation for actor 3 whose greeting reads tables with the expression ops: a word
/// written by `a6`, found by `c3`, a search past the last word to the `b8`, a word read by
/// `b4`, a search of a string table with an empty string to its `b8`, a `b4` with nothing
/// below its index (it reads the script's start); then bye.
fn table_conversation() -> (Vec<u8>, u32, u32) {
    let mut s = b"\xff\x03Ab\xf1x\xf2".to_vec();
    let (w, t) = (100u32, 110u32);
    s.extend([0xa6]);
    s.extend(lit32(w));
    s.extend([0x00, 0xa8, 0x07, 0xa7]);
    s.extend([0xa6, 0x03, 0xb2, 0xa8, 0x07, 0xa7]);
    for (var, op) in [
        (0x02, [0x03, 0xc3]),
        (0x04, [0x04, 0xc3]),
        (0x05, [0x01, 0xb4]),
    ] {
        s.extend([0xa6, var, 0xb2, 0xa8]);
        s.extend(lit32(w));
        s.extend(op);
        s.push(0xa7);
    }
    s.extend([0xa6, 0x06, 0xb2, 0xa8]);
    s.extend(lit32(t));
    s.extend([0x00, 0xb7, 0xa7]);
    s.extend([0xa6, 0x07, 0xb2, 0xa8, 0x01, 0xb4, 0xa7, b'O', b'k', 0xb6]);
    s.resize(w as usize, 0);
    s.extend([0x10, 0x00, 0x20, 0x00, 0xb8]);
    s.resize(t as usize, 0);
    s.extend(b"ab\0\0cd\0\xb8");
    (s, w, t)
}

#[test]
fn the_expression_ops_table_reads_are_checked_too() {
    let (s, w, _) = table_conversation();
    let d = ok(&s);
    let scripts: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 3),
        Decoded {
            bytes: &s,
            disasm: &d,
        },
    )]
    .into();
    trace::start(true);
    let end = talk_to(state(&[]), conversation_files(&s), 3, "");
    let events = trace::stop();
    assert!(scroll(&end).contains("Ok"), "{}", scroll(&end));
    let n = trace::check(&events, &scripts);
    assert_eq!(n.failures(), 0, "{n:?}");
    // c3 finds the written word (1 read), c3 reads both words to the b8, b4 reads one; b7
    // reads three strings (one empty) to the b8.
    assert_eq!(
        (n.table_writes, n.table_words, n.table_ends, n.table_strings),
        (1, 4, 2, 3),
        "{n:?}"
    );
    // The `b4` below the stack reads the script's start: the decoder marked it.
    assert_eq!(n.outside_reads, 1, "{n:?}");
    let mut unmarked = d.clone();
    unmarked.hazards.retain(|h| h.kind != HazardKind::Underflow);
    let bad: BTreeMap<(u8, u16), Decoded<'_>> = [(
        (b'a', 3),
        Decoded {
            bytes: &s,
            disasm: &unmarked,
        },
    )]
    .into();
    assert_eq!(trace::check(&events, &bad).table_mismatches, 1);
    // The written word is what c3 reads: without the write the bytes differ.
    let unwritten: Vec<Event> = events
        .iter()
        .filter(|e| !matches!(e, Event::WordWrite { .. }))
        .cloned()
        .collect();
    assert!(trace::check(&unwritten, &scripts).byte_mismatches > 0);
    // A word read where the disassembly has no word table is a mismatch.
    let mut wrong = d.clone();
    for it in &mut wrong.items {
        if it.offset == w {
            it.kind = Kind::Unreached;
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
    assert!(trace::check(&events, &bad).table_mismatches >= 5);
}

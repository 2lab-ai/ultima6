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
use super::decode::{Decoder, Plan, REVERSED, ref_entry};
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
    // print's index is past it and its namers (both of the `*` block's loops) are dropped; the
    // a1's code stays.
    let (s, x) = a_table_at_an_ifs_code(&[0xb1], b"zz\xb6");
    let d = ok(&s);
    assert!(matches!(item(&d, x).kind, Kind::Text(_)), "{:?}", shape(&d));
    assert!(!d.items.iter().any(|i| matches!(i.kind, Kind::Table(_))));
    for scan in [Scan::NoMatch, Scan::Answered] {
        assert!(
            d.hazards.contains(&Hazard {
                at: BODY + 19,
                kind: HazardKind::Dropped(scan, Reason::TableIndex)
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
        for reversed in [false, true] {
            REVERSED.with(|r| r.set(reversed));
            let d = ok(&s);
            REVERSED.with(|r| r.set(false));
            let what = format!("joined {joined}, reversed {reversed}");
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
fn a_landing_that_would_fail_in_a_table_stops_there_whichever_landing_comes_first() {
    // In script order the `a1`'s landing names the table first and the block's loops stop
    // there; the other way round they read the table's bytes as code first and fail. They are
    // tried again once the table is kept, and stop there too.
    let (s, t) = a_table_a_failing_landing_reaches();
    for reversed in [false, true] {
        REVERSED.with(|r| r.set(reversed));
        let d = ok(&s);
        REVERSED.with(|r| r.set(false));
        assert!(
            matches!(item(&d, t).kind, Kind::Table(_)),
            "reversed {reversed}: {:?}",
            shape(&d)
        );
        assert_eq!(
            d.hazards,
            [Hazard {
                at: t,
                kind: HazardKind::IntoData
            }],
            "reversed {reversed}"
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
    for reversed in [false, true] {
        REVERSED.with(|r| r.set(reversed));
        let d = ok(&s);
        REVERSED.with(|r| r.set(false));
        for at in [t, u] {
            assert!(
                matches!(item(&d, at).kind, Kind::Table(_)),
                "reversed {reversed}: {:?}",
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
            "reversed {reversed}"
        );
    }
}

/// The decode of `s` with the landings taken in script order and in the reverse order: its
/// items, its hazards (as a set) and its catalog.
fn both_orders(s: &[u8]) -> [(Vec<Item>, Vec<String>, String); 2] {
    [false, true].map(|reversed| {
        // The catalog decodes again: in the same order.
        REVERSED.with(|r| r.set(reversed));
        let d = ok(s);
        let catalog = Catalog::build([(b'a', 7, s)]).to_json();
        REVERSED.with(|r| r.set(false));
        let mut hazards: Vec<String> = d.hazards.iter().map(|h| format!("{h:?}")).collect();
        hazards.sort();
        (d.items, hazards, catalog)
    })
}

#[test]
fn which_of_a_table_and_a_landings_code_wins_does_not_depend_on_the_landing_order() {
    // Scripts where a table meets a landing's code (and the landings' code does not overlap,
    // where the earlier landing wins: the module doc). Each decodes the same either way.
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
    for (n, s) in scripts.iter().enumerate() {
        let [a, b] = both_orders(s);
        assert!(a == b, "script {n}: {:?}\n{:?}", a.1, b.1);
    }
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

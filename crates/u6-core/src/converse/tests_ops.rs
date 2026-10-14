//! One test per op on hand-built scripts: evaluator 0x1130a2, executor 0x11404c, text runner
//! 0x1148f0 / keyword blocks 0x1149d6, input ops of 0x114afa, set-up 0x1125b8.

use u6_formats::position::Position;

use super::eval::{NOT_FOUND, eval};
use super::run::run_text;
use super::tests::{chars, run, script_files, scroll, state};
use super::vars::StrRef;
use super::{SCRIPT_LEN, begin, int_var, string_var};
use crate::engine::{Engine, InputEvent};
use crate::files::Files;
use crate::state::State;

const NONE: Vec<InputEvent> = Vec::new();

/// Evaluates the expression at the start of `s`'s script.
fn ev_in(s: State) -> (u32, State) {
    run!(s, Files::default(), NONE, |cx| eval(cx).await)
}

fn ev(script: &[u8]) -> u32 {
    ev_in(state(script)).0
}

/// A little-endian `d2` address literal.
fn addr(a: u32) -> Vec<u8> {
    [&[0xd2][..], &a.to_le_bytes()].concat()
}

fn with_at(script: &[u8], at: usize, data: &[u8]) -> Vec<u8> {
    let mut v = script.to_vec();
    v.resize(at.max(v.len()), 0);
    v.splice(at..at, data.iter().copied());
    v
}

/// Runs the text runner from the start of `s`'s script for NPC 9 with `keys`.
fn text_in(s: State, keys: &str) -> State {
    run!(s, script_files(), chars(keys), |cx| run_text(cx, 9).await).1
}

fn text(script: &[u8]) -> State {
    text_in(state(script), "")
}

// ---- 0x1130a2 -----------------------------------------------------------------------------------

#[test]
fn literals_and_plain_bytes() {
    assert_eq!(ev(&[0x05, 0xa7]), 5);
    assert_eq!(ev(&[0xd3, 0xf0, 0xa7]), 0xf0);
    assert_eq!(ev(&[0xd4, 0x34, 0x12, 0xa7]), 0x1234);
    assert_eq!(ev(&[0xd2, 1, 2, 3, 4, 0xa8]), 0x0403_0201);
    assert_eq!(ev(&[0x99, 0xa7]), 0x99, "a byte that is no op is a value");
    assert_eq!(ev(&[0xa7]), 0, "empty expression");
    let (_, s) = ev_in(state(&[1, 0xa7, 0x55]));
    assert_eq!(s.converse.pc, 2, "the terminator is consumed");
}

#[test]
fn comparisons_are_unsigned_32_bit() {
    let big = [0xd2, 0xff, 0xff, 0xff, 0xff];
    assert_eq!(
        ev(&[&big[..], &[1, 0x81, 0xa7]].concat()),
        1,
        "0xffffffff > 1"
    );
    assert_eq!(ev(&[&big[..], &[1, 0x83, 0xa7]].concat()), 0, "not < 1");
    assert_eq!(ev(&[3, 3, 0x82, 0xa7]), 1);
    assert_eq!(ev(&[2, 3, 0x82, 0xa7]), 0);
    assert_eq!(ev(&[3, 3, 0x84, 0xa7]), 1);
    assert_eq!(ev(&[4, 3, 0x84, 0xa7]), 0);
    assert_eq!(ev(&[3, 4, 0x85, 0xa7]), 1);
    assert_eq!(ev(&[3, 3, 0x86, 0xa7]), 1);
    assert_eq!(ev(&[3, 4, 0x86, 0xa7]), 0);
}

#[test]
fn arithmetic_and_logic() {
    assert_eq!(ev(&[3, 5, 0x90, 0xa7]), 8);
    assert_eq!(ev(&[3, 5, 0x91, 0xa7]), 0xffff_fffe);
    assert_eq!(ev(&[6, 7, 0x92, 0xa7]), 42);
    assert_eq!(ev(&[7, 2, 0x93, 0xa7]), 3, "unsigned quotient");
    assert_eq!(ev(&[0, 0, 0x94, 0xa7]), 0);
    assert_eq!(ev(&[0, 9, 0x94, 0xa7]), 1);
    assert_eq!(ev(&[3, 9, 0x95, 0xa7]), 1);
    assert_eq!(ev(&[3, 0, 0x95, 0xa7]), 0);
    // Division by zero prints the error and leaves the dividend.
    let (v, s) = ev_in(state(&[7, 0, 0x93, 0xa7]));
    assert_eq!(v, 7);
    assert_eq!(scroll(&s), "[DivideError]");
    // Two values left: "Error occured.", the bottom one is the result.
    let (v, s) = ev_in(state(&[1, 2, 0xa7]));
    assert_eq!((v, scroll(&s)), (1, "[ErrorOccured]".into()));
}

#[test]
fn variables_and_strings() {
    let mut s = state(&[3, 0xb2, 0xa7]);
    s.converse.ints[3] = 77;
    assert_eq!(ev_in(s).0, 77);
    // Two string slots compared without case.
    let mut s = state(&[0, 0xb3, 1, 0xb3, 0x86, 0xa7]);
    s.converse.strs[0] = StrRef::Buffer(0);
    s.converse.strs[1] = StrRef::Buffer(1);
    s.converse.buffers[0] = b"Hello".to_vec();
    s.converse.buffers[1] = b"hELLO".to_vec();
    assert_eq!(ev_in(s.clone()).0, 1);
    s.converse.buffers[1] = b"Help".to_vec();
    assert_eq!(ev_in(s).0, 0);
}

#[test]
fn talk_flags_bits_and_the_self_operand() {
    let mut s = state(&[5, 2, 0xab, 0xa7]);
    s.actors.attrs.talk_flags[5] = 0b100;
    assert_eq!(ev_in(s.clone()).0, 1);
    s.converse.script[1] = 1;
    assert_eq!(ev_in(s.clone()).0, 0);
    // 0xeb = the NPC talked to.
    s.converse.script[..2].copy_from_slice(&[0xeb, 2]);
    s.converse.npc = 5;
    assert_eq!(ev_in(s).0, 1);
}

#[test]
fn random_numbers_use_the_game_rng() {
    let mut s = state(&[1, 6, 0xa0, 0xa7]);
    s.rng.seed(0x1234);
    let mut rng = s.rng;
    let (v, after) = ev_in(s);
    assert_eq!(v, u32::from(rng.range(1, 6)));
    assert!((1..=6).contains(&v));
    assert_eq!(after.rng, rng, "one draw");
    assert_eq!(ev(&[3, 3, 0xa0, 0xa7]), 3);
}

#[test]
fn script_tables() {
    // 0xb4: word `index` of the table at an address (sign-extended).
    let table = [0x11, 0x11, 0x22, 0x22, 0xfe, 0xff];
    let s = with_at(&[&addr(0x40)[..], &[2, 0xb4, 0xa7]].concat(), 0x40, &table);
    assert_eq!(ev(&s), 0xffff_fffe);
    // 0xb7: index of the first string whose first four letters match a word of a slot.
    let strings = b"apple\0banana\0cherry\0\xb8";
    let code = [&addr(0x40)[..], &[0, 0xb7, 0xa7]].concat();
    let mut st = state(&with_at(&code, 0x40, strings));
    st.converse.strs[0] = StrRef::Buffer(0);
    st.converse.buffers[0] = b"I want CHERRIES".to_vec();
    let (v, after) = ev_in(st.clone());
    assert_eq!(v, 2);
    assert_eq!(
        after.converse.pc as usize,
        code.len(),
        "read position restored"
    );
    st.converse.buffers[0] = b"nothing here".to_vec();
    assert_eq!(ev_in(st).0, 3, "not found = number of entries");
    // 0xc3: index of a word variable's value in a word table.
    let code = [&addr(0x40)[..], &[4, 0xc3, 0xa7]].concat();
    let mut st = state(&with_at(&code, 0x40, &[0x10, 0, 0x20, 0, 0xb8]));
    st.converse.ints[4] = 0x20;
    assert_eq!(ev_in(st.clone()).0, 1);
    st.converse.ints[4] = 0x30;
    assert_eq!(ev_in(st).0, NOT_FOUND);
}

#[test]
fn constant_and_stub_ops() {
    assert_eq!(ev(&[1, 2, 0xbc, 0xa7]), 100);
    assert_eq!(ev(&[1, 2, 0xbd, 0xa7]), 100);
    assert_eq!(ev(&[1, 2, 0xbb, 0xa7]), 0, "object count seam: 0");
    assert_eq!(
        ev(&[1, 2, 3, 0x9f, 0xa7]),
        0,
        "carries-object seam: not carried"
    );
    assert_eq!(ev(&[2, 3, 0xc7, 0xa7]), NOT_FOUND, "no member carries it");
    // 0xc0 asks for an item; a key that picks nothing leaves the selection as the cursor code
    // leaves it with nothing under the pointer (-1, -1).
    let mut st = state(&[0xc0, 0xa7]);
    st.ui.input.selection = -1;
    st.ui.input.target = (-1, -1);
    let (v, s) = run!(st, Files::default(), chars("x"), |cx| { eval(cx).await });
    assert_eq!(v, NOT_FOUND);
    assert_eq!(scroll(&s), "[SelectItem]");
}

#[test]
fn object_ops() {
    // 0xc1: location word of an object; the byte after the next one is resolved in place.
    let mut s = state(&[5, 0xc1, 0xa7, 0xeb]);
    s.objects.location[5] = Position {
        x: 0x123,
        y: 0x45,
        z: 2,
    };
    s.converse.npc = 9;
    let packed = s.objects.location[5].to_packed();
    let (v, after) = ev_in(s);
    assert_eq!(v as u16, u16::from_le_bytes([packed[0], packed[1]]));
    assert_eq!(after.converse.script[3], 9);
    // 0xc2: object type.
    let mut s = state(&[0xd4, 0x2c, 0x01, 0xc2, 0xa7]);
    s.objects.type_frame[300] = 0x1234;
    assert_eq!(ev_in(s).0, 0x234);
    // 0x9d: riding (type 0x1af).
    let mut s = state(&[5, 0x9d, 0xa7]);
    s.objects.type_frame[5] = 0x1af;
    assert_eq!(ev_in(s).0, 1);
    // 0x9b: weight = quantity × weight, ×10 for types not counted (seam: none counted).
    let mut s = state(&[0x10, 4, 0x9b, 0xa7]);
    s.tables.tileflags.weight = vec![0; 0x20];
    s.tables.tileflags.weight[0x10] = 3;
    assert_eq!(ev_in(s).0, 120);
    // 0x9a: what an actor can still carry: strength × 200 − carried × 10.
    let mut s = state(&[5, 0x9a, 0xa7]);
    s.actors.attrs.strength[5] = 10;
    assert_eq!(ev_in(s).0, 2000);
}

#[test]
fn actor_ops() {
    let mut s = state(&[2, 0xc6, 0xa7]);
    assert_eq!(ev_in(s.clone()).0, 1, "member on screen");
    s.actors.attrs.status[2] |= 0x10;
    assert_eq!(ev_in(s).0, 0, "dead");
    assert_eq!(ev(&[2, 0xd7, 0xa7]), 1);
    let mut s = state(&[5, 0xda, 0xa7]);
    s.actors.attrs.level[5] = 1;
    s.actors.attrs.hp[5] = 10;
    assert_eq!(ev_in(s.clone()).0, 1, "wounded");
    s.actors.attrs.hp[5] = 30;
    assert_eq!(ev_in(s).0, 0);
    let mut s = state(&[5, 0xdc, 0xa7]);
    s.actors.attrs.status[5] = 0x08;
    assert_eq!(ev_in(s).0, 8, "poisoned: the bit, not 1");
    // 0xdd: party member by index; flag 0 counts members on screen only.
    assert_eq!(ev(&[1, 1, 0xdd, 0xa7]), 2);
    assert_eq!(ev(&[5, 1, 0xdd, 0xa7]), 0);
    let mut s = state(&[1, 0, 0xdd, 0xa7]);
    s.objects.location[2] = Position { x: 300, y: 0, z: 0 };
    assert_eq!(ev_in(s).0, 0, "the second member is off screen");
}

#[test]
fn stat_ops_cap() {
    let mut s = state(&[1, 20, 0xe0, 0xa7]);
    s.actors.attrs.exp[1] = 9990;
    let (v, after) = ev_in(s);
    assert_eq!((v, after.actors.attrs.exp[1]), (9999, 9999));
    let mut s = state(&[1, 2, 0xe1, 0xa7]);
    s.actors.attrs.level[1] = 3;
    assert_eq!(ev_in(s).0, 5);
    for (op, field) in [(0xe2u8, 0usize), (0xe3, 1), (0xe4, 2)] {
        let mut s = state(&[1, 5, op, 0xa7]);
        let a = &mut s.actors.attrs;
        [&mut a.strength, &mut a.intelligence, &mut a.dexterity][field][1] = 28;
        assert_eq!(ev_in(s).0, 30, "op {op:#x} caps at 30");
    }
}

#[test]
fn join_and_leave_ops() {
    let (v, s) = run!(state(&[3, 0xca, 0xa7]), script_files(), NONE, |cx| eval(cx)
        .await);
    assert_eq!(v, 0);
    let p = &s.actors.party;
    assert_eq!((p.size, p.members[2], p.names[2].as_str()), (3, 3, "Ann"));
    assert_eq!(
        s.actors.attrs.status[3] & 0xe0,
        0xc0,
        "member, alignment 0x40"
    );
    assert_eq!(
        (s.actors.worktype[3], s.actors.attrs.combat_mode[3]),
        (1, 3)
    );
    let mut full = state(&[3, 0xca, 0xa7]);
    full.actors.party.size = 8;
    assert_eq!(run!(full, script_files(), NONE, |cx| eval(cx).await).0, 2);
    let mut ship = state(&[3, 0xca, 0xa7]);
    ship.actors.party.leader_index = 2;
    assert_eq!(run!(ship, script_files(), NONE, |cx| eval(cx).await).0, 1);
    // Leave: the roster closes up.
    let mut s = state(&[1, 0xcc, 0xa7]);
    s.actors.attrs.status[1] = 0xc0;
    let (v, s) = ev_in(s);
    assert_eq!(v, 0);
    assert_eq!((s.actors.party.size, s.actors.party.members[0]), (1, 2));
    assert_eq!(s.actors.party.names[0], "Friend");
    assert_eq!((s.actors.attrs.status[1], s.actors.worktype[1]), (0, 0x8f));
    assert_eq!(ev(&[7, 0xcc, 0xa7]), 2, "not a member");
}

// ---- 0x11404c via the text runner 0x1148f0 -----------------------------------------------------

#[test]
fn text_prints_and_stops() {
    let s = text(b"Hi\nyou\0more");
    assert_eq!(scroll(&s), "Hi\nyou");
    assert_eq!(s.converse.pc, 6, "left on the stop byte");
    let s = text(b"A\x01B");
    assert_eq!(scroll(&s), "A[ControlChar]");
    let s = text(b"A\xb6B");
    assert_eq!(scroll(&s), "A");
    assert!(s.converse.bye);
    let s = text(&[b'A', 0xd1, b'B', 0]);
    assert_eq!(scroll(&s), "A[UnknownCommand]B");
}

#[test]
fn if_else_endif_and_jump() {
    let prog = |c: u8| vec![0xa1, c, 0xa7, b'Y', 0xa3, b'N', 0xa2, b'.', 0];
    assert_eq!(scroll(&text(&prog(1))), "Y.");
    assert_eq!(scroll(&text(&prog(0))), "N.");
    // A false condition steps over a jump's operand bytes (they hold a2 here).
    let s = text(&[
        0xa1, 0, 0xa7, 0xb0, 0xa2, 0xa2, 0xa2, 0xa2, 0xa3, b'N', 0xa2, 0,
    ]);
    assert_eq!(scroll(&s), "N");
    // Condition -1: the op fails and the runner goes on into the "then" text.
    assert_eq!(
        scroll(&text(&[0xa1, 0xd4, 0xff, 0xff, 0xa7, b'Y', 0xa2, 0])),
        "Y"
    );
    // Jump: low word of the operand.
    assert_eq!(
        scroll(&text(&[0xb0, 8, 0, 0, 0, b'X', b'X', b'X', b'J', 0])),
        "J"
    );
}

#[test]
fn flags_karma_and_heal_ops() {
    let mut s = state(&[0xa4, 0xeb, 0xa7, 3, 0xa7, 0xa5, 9, 0xa7, 0, 0xa7, 0]);
    s.converse.npc = 9;
    s.actors.attrs.talk_flags[9] = 1;
    let s = text_in(s, "");
    assert_eq!(
        s.actors.attrs.talk_flags[9], 0b1000,
        "bit 3 set on the NPC, bit 0 cleared"
    );
    let mut s = state(&[0xc4, 10, 0xa7, 0]);
    s.actors.karma = 95;
    assert_eq!(text_in(s, "").actors.karma, 99);
    let mut s = state(&[0xc5, 10, 0xa7, 0]);
    s.actors.karma = 5;
    assert_eq!(text_in(s.clone(), "").actors.karma, 0);
    s.actors.karma = 50;
    assert_eq!(text_in(s, "").actors.karma, 40);
    let mut s = state(&[0xd9, 5, 0xa7, 0xdb, 5, 0xa7, 0]);
    s.actors.attrs.level[5] = 2;
    s.actors.attrs.status[5] = 0x09;
    let s = text_in(s, "");
    assert_eq!((s.actors.attrs.hp[5], s.actors.attrs.status[5]), (60, 1));
}

#[test]
fn assignment_op() {
    let s = text(&[0xa6, 4, 0xb2, 0xa8, 7, 2, 0x90, 0xa7, 0]);
    assert_eq!(int_var(&s, 4), 9);
    // String slot from another slot; the expression's a7 is then read as a no-op.
    let mut st = state(&[0xa6, 1, 0xb3, 0xa8, 0, 0xb3, 0xa7, b'.', 0]);
    st.converse.strs[..2].copy_from_slice(&[StrRef::Buffer(0), StrRef::Buffer(1)]);
    st.converse.buffers[0] = b"copy".to_vec();
    let s = text_in(st.clone(), "");
    assert_eq!(
        (string_var(&s, 1), scroll(&s)),
        (b"copy".to_vec(), ".".into())
    );
    // String slot from string 2 of a table.
    let code = [&[0xa6, 1, 0xb3, 0xa8][..], &addr(0x40), &[2, 0xa7, b'.', 0]].concat();
    st.converse.script = with_at(&code, 0x40, b"aa\0bb\0cc\0");
    st.converse.script.resize(SCRIPT_LEN, 0);
    let s = text_in(st, "");
    assert_eq!(
        (string_var(&s, 1), scroll(&s)),
        (b"cc".to_vec(), ".".into())
    );
    // Word 1 of a script table overwritten (the index expression ends with the a8).
    let code = [
        &[0xa6][..],
        &addr(0x40),
        &[1, 0xa8, 0xd4, 0x34, 0x12, 0xa7, 0],
    ]
    .concat();
    let s = text(&with_at(&code, 0x40, &[0, 0, 0, 0]));
    assert_eq!(&s.converse.script[0x42..0x44], &[0x34, 0x12]);
}

#[test]
fn print_op() {
    let mut st = state(&[0xb5, 0xd3, 1, 0xb3, 0xa7, b'.', 0]);
    st.converse.strs[1] = StrRef::Buffer(1);
    st.converse.buffers[1] = b"slot".to_vec();
    assert_eq!(scroll(&text_in(st, "")), "slot.");
    let code = [&[0xb5][..], &addr(0x40), &[1, 0xa7, b'.', 0]].concat();
    assert_eq!(scroll(&text(&with_at(&code, 0x40, b"aa\0bb\0"))), "bb.");
    assert_eq!(scroll(&text(&[0xb5, 0xd5, b'Z', 0])), "Z");
}

#[test]
fn name_ops_set_y() {
    let s = text(&[0xd8, 2, 0xa7, 0]);
    assert_eq!(s.converse.strs[0x22], StrRef::PartyName(1));
    assert_eq!(string_var(&s, 0x22), b"Friend");
    let s = text(&[0xdf, 1, 0xa7, 0]);
    assert_eq!(s.converse.strs[0x22], StrRef::Text(super::Msg::You));
    let s = text(&[0xd8, 3, 0xa7, 0]);
    assert_eq!(
        (s.converse.strs[0x22], string_var(&s, 0x22)),
        (StrRef::NameBuf, b"Ann".to_vec())
    );
}

#[test]
fn worktype_op() {
    let mut s = state(&[0xcd, 5, 0xa7, 0xd3, 0x91, 0xa7, 0]);
    s.objects.type_frame[5] = 0x188 | (3 << 10);
    let s = text_in(s, "");
    assert_eq!((s.actors.worktype[5], s.actors.attrs.status[5]), (0x91, 4));
    assert_eq!(s.objects.type_frame[5], 0x182 | (3 << 10));
    let mut s = state(&[0xcd, 5, 0xa7, 0xd3, 0x95, 0xa7, 0]);
    s.objects.type_frame[5] = 0x188;
    assert_eq!(
        text_in(s, "").objects.type_frame[5],
        0x188,
        "0x95 keeps the type"
    );
}

#[test]
fn waiting_ops() {
    // 0xcb waits for one key.
    let s = text_in(state(&[0xcb, b'A', 0]), "k");
    assert_eq!(scroll(&s), "A");
    // 0xd0 refreshes the view n × 20 times (the seam waits one VBlank each).
    let mut e = Engine::with_game(
        state(&[0xd0, 1, 0xa7, 0]),
        Files::default(),
        |st, io| async move {
            run_text(&super::Cx::new(&st, &io), 9).await;
        },
    );
    let mut ticks = 0;
    while !e.finished() && ticks < 100 {
        e.tick(&[], 16);
        ticks += 1;
    }
    assert_eq!(ticks, 21);
    // 0x9e advances the clock to 5 o'clock (a whole day when it is 5 already).
    let mut s = state(&[0x9e, 0]);
    s.actors.calendar.hour = 3;
    assert_eq!(text_in(s, "").actors.calendar.hour, 5);
}

#[test]
fn seam_ops_reach_their_stubs() {
    // The ops whose callee is still outside milestone 1 (the object ops 0xb9 / 0xba / 0xbe /
    // 0xc8 / 0xc9 call the ported objects / ui code since the integration; 0xd6 calls the
    // ported resurrection 0x11785e of magic since m2).
    for (script, id) in [
        (vec![0xbf, 5, 0xa7, 0], "0x128ce8"),
        (vec![0x9c, 5, 0xa7, 0], "0x12f6ee"),
    ] {
        let mut e = Engine::with_game(state(&script), Files::default(), |st, io| async move {
            run_text(&super::Cx::new(&st, &io), 9).await;
        });
        e.tick(&[], 16);
        assert!(e.finished());
        assert!(
            e.stub_hits().keys().any(|k| k.starts_with(id)),
            "{id}: {:?}",
            e.stub_hits()
        );
    }
}

// ---- keyword blocks 0x1149d6 -------------------------------------------------------------------

fn block() -> Vec<u8> {
    let mut v = vec![0xef];
    v.extend_from_slice(b"name,job");
    v.push(0xf6);
    v.push(b'A');
    v.push(0xef);
    v.extend_from_slice(b"fare");
    v.push(0xf6);
    v.push(b'B');
    v.push(0xef);
    v.push(b'*');
    v.push(0xf6);
    v.push(b'C');
    v.extend_from_slice(&[0xee, b'.', 0]);
    v
}

fn answer(input: &[u8], answered: bool) -> String {
    let mut s = state(&block());
    s.converse.input = input.to_vec();
    s.converse.answered = answered;
    scroll(&text_in(s, ""))
}

#[test]
fn keyword_blocks_answer_the_first_match() {
    assert_eq!(
        answer(b"my JOB", false),
        "\nA.",
        "second keyword of the first block"
    );
    assert_eq!(answer(b"", false), "\nB.", "empty input = the bye word");
    assert_eq!(
        answer(b"xyz", false),
        "\nC",
        "* matches anything; its answer stops at ee"
    );
    assert_eq!(
        answer(b"name", true),
        ".",
        "already answered: blocks skipped to ee"
    );
}

// ---- input ops of 0x114afa ---------------------------------------------------------------------

/// A conversation with NPC 3 whose script is `body` after the header and greeting.
fn talk_script(body: &[u8], keys: &str) -> State {
    let mut script = b"\xff\x03Ann\xf1a woman\xf2".to_vec();
    script.extend_from_slice(body);
    let mut a: Vec<Option<Vec<u8>>> = vec![None; 4];
    a[3] = Some(super::tests::packed(&script));
    let f = super::tests::files(&[("talkfile.a", super::tests::lib(&a))]);
    run!(state(&[]), f, chars(keys), |cx| begin(cx, 0, 3).await).1
}

#[test]
fn ask_loop_with_look_and_bye() {
    let mut body = b"Hi $P.".to_vec();
    let ask = b"\xff\x03Ann\xf1a woman\xf2".len() + body.len();
    body.extend_from_slice(b"\xf7\xefname\xf6I am $N.\xeffare\xf6Bye.\xb6\xee\xb0");
    body.extend_from_slice(&(ask as u32).to_le_bytes());
    let s = talk_script(&body, "name\npeek\nfarewell\n");
    let t = scroll(&s);
    assert!(
        t.starts_with("[YouSeeFirst]a woman\nHi Hero.\nsay:name\n\nI am Ann.\nsay:peek\n"),
        "{t}"
    );
    assert!(t.contains("[YouSee]a woman\nsay:farewell\n\nBye."), "{t}");
    assert!(!s.converse.active && !s.converse.bye);
}

#[test]
fn input_ops_fill_variables() {
    let s = talk_script(b"\xf9\x05\xb3$5\xb6", "typed\n");
    assert!(scroll(&s).ends_with("typed\n\ntyped"), "{}", scroll(&s));
    let s = talk_script(b"\xfb\x04\xb2#4\xb6", "42\n");
    assert!(scroll(&s).ends_with("42\n\n42"), "{}", scroll(&s));
    let s = talk_script(b"\xfb\x04\xb2#4\xb6", "x\n");
    assert!(scroll(&s).ends_with("x\n\n0"), "not a number: 0");
    let s = talk_script(b"\xfc\x04\xb2#4\xb6", "x7");
    assert!(
        scroll(&s).ends_with("=7\n7"),
        "a non-digit is ignored: {}",
        scroll(&s)
    );
    let s = talk_script(b"\xfa$Z\xb6", "q");
    assert!(scroll(&s).ends_with("=Q\nQ"), "{}", scroll(&s));
    let s = talk_script(b"\xf8yn\xefy\xf6Yes\xb6\xefn\xf6No\xb6\xee", "xn");
    assert!(scroll(&s).ends_with("=N\n\nNo"), "{}", scroll(&s));
    let s = talk_script(b"\xcbW\xb6", "k");
    assert!(scroll(&s).ends_with("\nW"), "{}", scroll(&s));
    let s = talk_script(b"\xff", "");
    assert!(scroll(&s).ends_with("[YouSeeFirst]a woman\n"));
    let s = talk_script(b"\xf3", "");
    assert!(scroll(&s).ends_with("[LogicalError]"));
}

// ---- 0x1125b8 ---------------------------------------------------------------------------------

#[test]
fn set_up_variables_and_write_back() {
    let probe = |hour: u8, female: bool| {
        let mut s = state(&[]);
        s.actors.calendar.hour = hour;
        s.actors.objlist_tail[0x80] = u8::from(female);
        s.actors.objlist_tail[0x5e..0x60].copy_from_slice(&[0x00, 0x01]);
        s.actors.quest_flag_copy = 1;
        let mut a: Vec<Option<Vec<u8>>> = vec![None; 4];
        let body = b"\xff\x03Ann\xf1x\xf2$G $T $P $N #H\xa6\x15\xb2\xa8\x07\xa7\xa6\x1a\xb2\xa8\x00\xa7\xb6";
        a[3] = Some(super::tests::packed(body));
        let f = super::tests::files(&[("talkfile.a", super::tests::lib(&a))]);
        run!(s, f, NONE, |cx| begin(cx, 0, 3).await).1
    };
    let s = probe(11, false);
    assert!(
        scroll(&s).ends_with("[Milord] [Morning] Hero Ann 11"),
        "{}",
        scroll(&s)
    );
    assert_eq!(
        &s.actors.objlist_tail[0x5e..0x60],
        &[0, 7],
        "var 0x15 written back"
    );
    assert_eq!(s.actors.quest_flag_copy, 0, "var 0x1a written back");
    assert!(scroll(&probe(12, true)).ends_with("[Milady] [Afternoon] Hero Ann 12"));
    assert!(scroll(&probe(18, false)).contains("[Evening]"));
}

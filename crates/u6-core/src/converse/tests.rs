//! Unit tests on synthetic data: harness, text extraction, loaders, helpers, the Talk flow.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use u6_formats::position::Position;

use super::party::{name_from_script, on_screen};
use super::run::match_keyword;
use super::script::{ScriptDst, load_portrait, load_script};
use super::text::{ConverseText, MESSAGES, Msg};
use super::{Cx, SCRIPT_LEN, talk};
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::state::State;

/// Synthetic placeholder messages (not the game's text): `[Name]` for each message, plus the
/// few whose shape the code depends on.
pub(super) fn text() -> ConverseText {
    ConverseText::from_pairs(MESSAGES.iter().map(|&(m, _)| {
        let t: &[u8] = match m {
            Msg::Delimiters => b" ",
            Msg::Bye => b"farewell",
            Msg::Look => b"peek",
            Msg::Newline => b"\n",
            Msg::TwoNewlines => b"\n\n",
            Msg::FormatNumber => b"=%d\n",
            Msg::FormatString => b"=%s\n",
            Msg::FormatChar => b"=%c",
            Msg::YouSay => b"\nsay:",
            Msg::ConverseFile => b"talkfile.?",
            Msg::PortraitFile => b"facefile.?",
            Msg::WordEnds => b" ,.",
            _ => return (m, format!("[{m:?}]").into_bytes()),
        };
        (m, t.to_vec())
    }))
}

pub(super) use crate::main_loop::test_support::test_ui;

/// A state with the synthetic messages, `script` in the script buffer, a party of actors 1 and
/// 2 (leader 1 at (100, 100, 0)) and the view centred there.
pub(super) fn state(script: &[u8]) -> State {
    let mut s = State::empty();
    s.ui = test_ui();
    s.converse.text = Some(text());
    s.converse.script = script.to_vec();
    s.converse.script.resize(SCRIPT_LEN, 0);
    let p = &mut s.actors.party;
    p.members[..2].copy_from_slice(&[1, 2]);
    p.size = 2;
    p.names = vec!["Hero".into(), "Friend".into()];
    s.actors.objlist_tail[0x79] = 0xff;
    s.actors.attrs.status[1] = 0x80;
    s.actors.attrs.status[2] = 0x80;
    for a in [1usize, 2] {
        s.objects.location[a] = Position {
            x: 100,
            y: 100,
            z: 0,
        };
    }
    s.map.centre = Position {
        x: 100,
        y: 100,
        z: 0,
    };
    s
}

pub(super) fn chars(text: &str) -> Vec<InputEvent> {
    text.chars()
        .map(|c| match c {
            '\n' => InputEvent::Key(Key::Enter),
            c => InputEvent::Key(Key::Char(c)),
        })
        .collect()
}

/// Runs `$body` (with `$cx: &Cx`) as the game of an engine over `$state` and `$files`, feeding
/// `$keys` one per tick; evaluates to (result, final state).
macro_rules! run {
    ($state:expr, $files:expr, $keys:expr, |$cx:ident| $body:expr) => {{
        let out = std::rc::Rc::new(std::cell::RefCell::new(None));
        let o = std::rc::Rc::clone(&out);
        let mut e = crate::engine::Engine::with_game($state, $files, move |st, io| async move {
            let $cx = &super::Cx::new(&st, &io);
            let r = $body;
            *o.borrow_mut() = Some(r);
        });
        for k in $keys {
            e.tick(&[k], 16);
        }
        for _ in 0..400 {
            if e.finished() {
                break;
            }
            e.tick(&[], 16);
        }
        assert!(e.finished(), "the game future ended");
        let r = out.borrow_mut().take().unwrap();
        (r, e.state().clone())
    }};
}
pub(super) use run;

/// The scroll as one string: the finished lines, then the line being written (its drawn part
/// and what waits in the line buffer).
pub(super) fn scroll(s: &State) -> String {
    let t = &s.ui.text;
    let mut lines = s.ui.messages.clone();
    let mut last: String = t.transcript_line.iter().map(|&b| char::from(b)).collect();
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    last.extend(pending.iter().map(|&b| char::from(b)));
    lines.push(last);
    lines.join("\n")
}

/// A U6Lib (u32 offsets) whose items are `items` (None = empty).
pub(super) fn lib(items: &[Option<Vec<u8>>]) -> Vec<u8> {
    let table = items.len() * 4;
    let mut body = Vec::new();
    let mut d = Vec::new();
    for it in items {
        let off = it.as_ref().map_or(0, |b| {
            let o = table + body.len();
            body.extend_from_slice(b);
            o as u32
        });
        d.extend_from_slice(&off.to_le_bytes());
    }
    d.extend(body);
    d
}

pub(super) fn packed(script: &[u8]) -> Vec<u8> {
    u6_formats::lzw::encode(script).unwrap()
}

fn raw(script: &[u8]) -> Vec<u8> {
    [&[0, 0, 0, 0][..], script].concat()
}

pub(super) fn files(entries: &[(&str, Vec<u8>)]) -> Files {
    Files::new(
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

// ---- text extraction ---------------------------------------------------------------------------

/// A hunk executable with 35 hunks: CODE/DATA hunks of the measured sizes for 2, 9, 10 and 33
/// (one marker byte `A` + n at the address of message n, zeros elsewhere), 4-byte CODE hunks
/// otherwise.
fn exe(hunk10_len: usize) -> Vec<u8> {
    let sizes: Vec<usize> = (0..35)
        .map(|n| match n {
            2 => 0x3a28,
            9 => 0x318,
            10 => hunk10_len,
            33 => 0x4f0c,
            _ => 4,
        })
        .collect();
    let base = |n: usize| match n {
        2 => 0x0010_4fb0u32,
        9 => 0x0011_25b8,
        10 => 0x0011_28d0,
        _ => 0x0015_36f0,
    };
    let mut f = Vec::new();
    let long = |f: &mut Vec<u8>, v: u32| f.extend_from_slice(&v.to_be_bytes());
    for v in [0x3f3, 0, 35, 0, 34] {
        long(&mut f, v);
    }
    for &n in &sizes {
        long(&mut f, (n / 4) as u32);
    }
    for (n, &len) in sizes.iter().enumerate() {
        let mut data = vec![0u8; len];
        for (i, &(_, addr)) in MESSAGES.iter().enumerate() {
            let hit = [2, 9, 10, 33].contains(&n) && addr >= base(n);
            if hit && ((addr - base(n)) as usize) < len {
                data[(addr - base(n)) as usize] = b'A' + i as u8;
            }
        }
        long(&mut f, if n == 33 { 0x3ea } else { 0x3e9 });
        long(&mut f, (len / 4) as u32);
        f.extend_from_slice(&data);
        for v in [0x3ec, 1, 0, 0, 0, 0x3f2] {
            long(&mut f, v);
        }
    }
    f
}

#[test]
fn messages_come_out_of_the_hunks_at_their_addresses() {
    let t = ConverseText::from_game(&exe(0x2958)).unwrap();
    for (i, &(m, _)) in MESSAGES.iter().enumerate() {
        assert_eq!(t.get(m), [b'A' + i as u8], "{m:?}");
    }
    let err = ConverseText::from_game(&exe(0x2954)).unwrap_err();
    assert!(err.contains("hunk 10"), "{err}");
    assert!(ConverseText::from_game(b"not a hunk file").is_err());
}

// ---- keyword test 0x1128d0 ----------------------------------------------------------------------

#[test]
fn keywords_match_any_word_prefix_case_folded_with_wildcards() {
    let d = b" ";
    assert!(match_keyword(b"name", b"what is thy NAME", 4, d));
    assert!(
        match_keyword(b"job", b"jobs", 3, d),
        "prefix of a longer word"
    );
    assert!(
        !match_keyword(b"job", b"jo", 3, d),
        "the NUL after a short word mismatches"
    );
    assert!(match_keyword(b"jo?", b"jo", 3, d), "? matches the NUL too");
    assert!(match_keyword(b"b?e", b"bye", 3, d));
    assert!(!match_keyword(b"name", b"nam e", 4, d));
    assert!(!match_keyword(b"x", b"   ", 1, d), "no word at all");
    assert!(
        match_keyword(b"NaMe", b"name", 4, d),
        "upper case in the keyword folds too"
    );
}

// ---- script and portrait loaders ----------------------------------------------------------------

pub(super) fn script_files() -> Files {
    let mut a: Vec<Option<Vec<u8>>> = vec![None; 0x63];
    a[3] = Some(packed(b"\xff\x03Ann\xf1look\xf2hi"));
    a[4] = Some(raw(b"\xff\x04Raw\xf1"));
    let mut b: Vec<Option<Vec<u8>>> = vec![None; 0x69];
    b[0] = Some(packed(b"\xff\x63Bea\xf1"));
    b[0x67] = Some(packed(b"\xff\xe5Obj\xf1"));
    files(&[("talkfile.a", lib(&a)), ("talkfile.b", lib(&b))])
}

#[test]
fn scripts_load_from_the_archive_and_item_the_code_picks() {
    let f = script_files();
    let mut s = state(&[]);
    s.converse.script = vec![0xaa; SCRIPT_LEN];
    assert_eq!(load_script(&mut s, &f, 3, ScriptDst::Main), 3);
    assert_eq!(&s.converse.script[..13], b"\xff\x03Ann\xf1look\xf2hi");
    assert_eq!(
        s.converse.script[13], 0xaa,
        "the rest keeps the previous bytes"
    );
    let mut other = Vec::new();
    assert_eq!(load_script(&mut s, &f, 4, ScriptDst::Other(&mut other)), 4);
    assert_eq!(&other[..6], b"\xff\x04Raw\xf1");
    assert_eq!(
        load_script(&mut s, &f, 0x63, ScriptDst::Other(&mut other)),
        0x63
    );
    assert_eq!(&other[2..5], b"Bea");
    assert_eq!(load_script(&mut s, &f, 5, ScriptDst::Main), 0, "empty item");
    assert_eq!(
        load_script(&mut s, &f, 0, ScriptDst::Main),
        0,
        "slot 0 never answers"
    );
    // Slots from 0xe0 talk only as the three object types (0x17e → item 0x67 of .b).
    s.objects.type_frame[0xe5] = 0x17e;
    assert_eq!(
        load_script(&mut s, &f, 0xe5, ScriptDst::Other(&mut other)),
        0xe5
    );
    assert_eq!(&other[2..5], b"Obj");
    s.objects.type_frame[0xe5] = 0x100;
    assert_eq!(load_script(&mut s, &f, 0xe5, ScriptDst::Main), 0);
}

#[test]
fn portraits_map_slots_to_archives() {
    let img = |v: u8| Some(packed(&vec![v; u6_formats::portrait::PORTRAIT_LEN]));
    let mut a = vec![None; 0x62];
    a[0] = img(10);
    a[4] = img(14);
    let mut b = vec![None; 0x61];
    b[0] = img(20);
    b[0x5f] = img(21);
    let z = vec![None, None, img(30)];
    let f = files(&[
        ("facefile.a", lib(&a)),
        ("facefile.b", lib(&b)),
        ("facefile.z", lib(&z)),
    ]);
    let mut s = state(&[]);
    let px = |s: &State, n: i16| load_portrait(s, &f, n).map(|p| p[0]);
    assert_eq!(px(&s, 0), Some(10), "slot 0 = item 0");
    assert_eq!(px(&s, 5), Some(14), "slot n = item n - 1");
    assert_eq!(px(&s, 0x63), Some(20), "from 0x62 on: portrait.b");
    s.actors.objlist_tail[0x81] = 3;
    assert_eq!(px(&s, 1), Some(30), "the avatar: portrait.z item n - 1");
    s.actors.objlist_tail[0x81] = 0;
    assert_eq!(px(&s, 1), None, "no avatar portrait");
    s.objects.type_frame[0xf0] = 0x16b;
    assert_eq!(
        px(&s, 0xf0),
        Some(21),
        "type 0x16b = slot 0xc2 = portrait.b item 0x5f"
    );
    assert_eq!(px(&s, 6), None, "empty item");
}

#[test]
fn script_names_cut_at_the_first_high_byte() {
    let a: Vec<Option<Vec<u8>>> = (0..0x63)
        .map(|i| (i == 3).then(|| packed(b"\xff\x03Ann\xf1")))
        .collect();
    let mut b: Vec<Option<Vec<u8>>> = vec![None; 0x10];
    b[0x6e - 0x63] = Some(packed(b"\xff\x6eDr Who\xf1"));
    b[1] = Some(packed(b"\xff\x64\xf1"));
    let f = files(&[("talkfile.a", lib(&a)), ("talkfile.b", lib(&b))]);
    let mut s = state(&[]);
    assert_eq!(name_from_script(&mut s, &f, 3), (b"Ann".to_vec(), false));
    assert_eq!(s.converse.name_buf, b"Ann");
    assert_eq!(
        name_from_script(&mut s, &f, 0x6e),
        (b"Dr.Who".to_vec(), false)
    );
    assert_eq!(
        name_from_script(&mut s, &f, 0x64),
        (Vec::new(), true),
        "empty name"
    );
    // No script: only the first byte is cleared, which ends the name after it.
    assert_eq!(name_from_script(&mut s, &f, 5), (Vec::new(), false));
}

// ---- 0x112bf2 ---------------------------------------------------------------------------------

#[test]
fn on_screen_needs_the_inner_window_or_a_vehicle() {
    let mut s = state(&[]);
    s.objects.location[9] = Position {
        x: 104,
        y: 96,
        z: 0,
    };
    assert!(on_screen(&s, 9));
    s.objects.location[9] = Position {
        x: 105,
        y: 100,
        z: 0,
    };
    assert!(!on_screen(&s, 9), "5 tiles away is outside");
    s.objects.location[9] = Position {
        x: 100,
        y: 95,
        z: 0,
    };
    assert!(!on_screen(&s, 9));
    // In a vehicle (leader index = party size) members count as present wherever they are.
    s.objects.location[2] = Position {
        x: 300,
        y: 300,
        z: 0,
    };
    assert!(!on_screen(&s, 2));
    s.actors.party.leader_index = 2;
    assert!(on_screen(&s, 2));
    assert!(!on_screen(&s, 9), "non-members still need the window");
}

// ---- Talk 0x1127c4 / 0x114afa refusals ----------------------------------------------------------

pub(super) fn talk_to(s: State, f: Files, target: i16, keys: &str) -> State {
    let (_, s) = run!(s, f, chars(keys), |cx| talk(cx, 0, target, true).await);
    s
}

#[test]
fn talk_refuses_in_the_original_order() {
    let f = script_files();
    // An object slot past the actors that is no shrine: "nothing!".
    let s = talk_to(state(&[]), f.clone(), 300, "");
    assert_eq!(scroll(&s), "[Nothing]");
    // Talking to oneself (actor 1 is the talker, party index 0).
    let s = talk_to(state(&[]), f.clone(), 1, "");
    assert!(
        scroll(&s).ends_with("[TalkingToYourself]"),
        "{}",
        scroll(&s)
    );
    // A party member off screen.
    let mut st = state(&[]);
    st.objects.location[2] = Position {
        x: 200,
        y: 100,
        z: 0,
    };
    let s = talk_to(st, f.clone(), 2, "");
    assert!(scroll(&s).ends_with("[NotOnScreen]"));
    // Solo mode for the second member.
    let mut st = state(&[]);
    st.actors.objlist_tail[0x79] = 1;
    let s = talk_to(st, f.clone(), 3, "");
    assert!(scroll(&s).ends_with("[NotInSolo]"));
    // A silent worktype, then asleep (status bit 1).
    let mut st = state(&[]);
    st.actors.worktype[3] = 0x12;
    assert!(scroll(&talk_to(st, f.clone(), 3, "")).ends_with("[NoResponseNpc]"));
    let mut st = state(&[]);
    st.actors.attrs.status[3] = 2;
    assert!(scroll(&talk_to(st, f.clone(), 3, "")).ends_with("[NoResponseNpc]"));
    // No script.
    let s = talk_to(state(&[]), f.clone(), 5, "");
    assert!(scroll(&s).ends_with("[FunnyNoResponse]"));
}

#[test]
fn shrines_talk_as_the_npc_in_their_object_word() {
    let f = script_files();
    let mut st = state(&[]);
    st.objects.type_frame[7] = 0x189;
    st.objects.qq[7] = 0x0300;
    let s = talk_to(st, f, 7, "");
    let text = scroll(&s);
    assert!(text.starts_with("[Shrine]"), "{text}");
    assert_eq!(s.converse.npc, 3);
    assert!(text.contains("hi"), "actor 3's greeting: {text}");
}

#[test]
fn stub_counters_record_the_seams_a_talk_reaches() {
    let f = script_files();
    let s = state(&[]);
    let calls = Rc::new(RefCell::new(0));
    let c = Rc::clone(&calls);
    let mut e = Engine::with_game(s, f, move |st, io| async move {
        talk(&Cx::new(&st, &io), 0, 3, false).await;
        *c.borrow_mut() += 1;
    });
    for k in chars("farewell\n") {
        e.tick(&[k], 16);
    }
    for _ in 0..8 {
        e.tick(&[], 16);
    }
    assert_eq!(*calls.borrow(), 1);
    let hits = e.stub_hits();
    // The text goes through the ui's printf (wired); the portrait panel is still a stub.
    assert!(!hits.keys().any(|k| k.starts_with("0x108658")), "{hits:?}");
    assert!(
        hits.keys().any(|k| k.starts_with("0x128ce8")),
        "portrait panel seam"
    );
}

#[test]
fn a_fault_left_by_an_earlier_script_does_not_block_the_next_talk() {
    let mut st = state(&[]);
    st.converse.fault = true;
    let s = talk_to(st, script_files(), 3, "");
    assert!(scroll(&s).contains("hi"), "{}", scroll(&s));
}

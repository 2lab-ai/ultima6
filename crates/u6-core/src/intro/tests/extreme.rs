//! A foreign `u` of extreme words, run through main in a debug build (every sum overflow-checked).
//!
//! What the image holds: every word i16::MIN or i16::MAX (four patterns: all MIN, all MAX, and
//! the two alternations), but for main's argument string; a compare chain at 0x10ac7a whose
//! one case (every passage) stores the scroll frame (blocks item 1 at 0x10, 0x10), lays the
//! passage box and its screen place out of MIN, MAX and the volume word A4+0x8d2, fills the
//! box and prints `%Y%X%s%d` with the volume word, the first volume-name pointer (set to
//! 0xfffffff0) and −1; and, for the runs that type, a working key table.
//!
//! What the files hold: the synthetic font, a zeroed `savegame/objlist`, and for every `.new`
//! the intro loads one 20-byte file whose table gives item 0 (empty) and item 1 the offset
//! 0xffffffff; no other item is in the table.
//!
//! What each run must reach: the opening alone runs 3000 ticks without an outcome; a creation
//! goes through the woods, the wagon and the farewell to the objlist (`Play { created: true
//! }`); the introduction, the transfer (Escape at the disk question) and the acknowledgments
//! each return to the menu, which takes Journey Onward (`Play { created: false }`), as does
//! Journey Onward at once; with u's own key table the menu has read every key sent and is
//! still waiting. No run may hit a stub but the clock's: the passage case is stepped
//! through to its end.
//!
//! Not covered: the passage cases of a real `u`, the save readers of the transfer, items with
//! pictures, and table words of `u` that only the drawing positions use (all covered by
//! the word sums but drawn off the screen here).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::intro::exe::UExe;
use crate::intro::{IntroOutcome, IntroState, LAUNCHER_ARGS, main};
use crate::state::State;

use super::synthetic::{font_bytes, keyboard};

/// Bytes of the synthetic image: up to the end of u's last hunk (0x11a010).
const IMAGE: usize = 0x1_a100;

/// The string main compares its argument with (mod.rs `JJS`).
const ARG: u32 = 0x0010_2c12;

/// The compare chain of 0x10ac08 (passages.rs `CHAIN`), its one case, the case's format and a
/// `jmp abs.l` stub to the print routine 0x1142f0 (out of `jsr d(pc)` reach).
const CHAIN: u32 = 0x0010_ac7a;
const CASE: u32 = 0x0010_ac80;
const FORMAT: u32 = 0x0010_ad40;
const STUB: u32 = 0x0010_ad60;
/// The fill routine 0x10f898 the case calls.
const FILL_VIEW: u32 = 0x0010_f898;

/// The volume-name pointer the case prints (`%s`): near the top of the address space.
const VOLUME_PTR: u32 = 0xffff_fff0;

/// A `d(pc)` displacement of the instruction word at `at` to `to`.
fn disp(at: u32, to: u32) -> u16 {
    to.wrapping_sub(at + 2) as u16
}

/// The case's words (instruction forms of passages.rs `run_case`).
fn case_words() -> Vec<u16> {
    let (max, min) = (i16::MAX as u16, i16::MIN as u16);
    let mut w: Vec<u16> = Vec::new();
    // The scroll frame: blocks item 1 at (0x10, 0x10).
    w.extend([
        0x397c, 0x10, 0x2cfc, 0x397c, 0x10, 0x2cfe, 0x397c, 1, 0x2d00,
    ]);
    // The box and its place: the volume word, MIN and MAX (move.w off(a4),d0; move.w d0 /
    // #imm,off(a4)).
    w.extend([
        0x302c, 0x08d2, 0x3940, 0x2d0a, 0x397c, min, 0x2d0c, 0x3940, 0x2d0e,
    ]);
    w.extend([0x397c, max, 0x2d10, 0x3940, 0x2d1c, 0x397c, min, 0x2d1e]);
    // moveq #5,d0; lea $2d02(a4),a0; jsr fill.
    w.extend([0x7005, 0x41ec, 0x2d02, 0x4eba]);
    w.push(disp(CASE + 2 * w.len() as u32 - 2, FILL_VIEW));
    // −1 (%d); the volume-name pointer of entry 0 (%s); the volume word twice (%X, %Y).
    w.extend([
        0x70ff, 0x3f00, 0x554f, 0x554f, 0x41ec, 0x08ac, 0x7000, 0x2eb0, 0x0800,
    ]);
    w.extend([0x302c, 0x08d2, 0x3f00, 0x3f00, 0x487a]);
    w.push(disp(CASE + 2 * w.len() as u32 - 2, FORMAT));
    w.push(0x4eba);
    w.push(disp(CASE + 2 * w.len() as u32 - 2, STUB));
    // lea 14(a7),a7; bra.w (to the tail).
    w.extend([0x4fef, 14, 0x6000, 0]);
    w
}

fn poke_words(e: &mut UExe, at: u32, words: &[u16]) {
    let b: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
    e.poke(at, &b);
}

/// The image of every word `words[0]`, `words[1]`, … with main's argument string, the passage
/// chain and case, and the volume-name pointers in place.
fn extreme(words: [i16; 2]) -> UExe {
    let flat = words
        .iter()
        .flat_map(|w| w.to_be_bytes())
        .cycle()
        .take(IMAGE)
        .collect();
    let mut e = UExe::from_flat(flat);
    let mut arg = LAUNCHER_ARGS[1].to_vec();
    arg.push(0);
    e.poke(ARG, &arg);
    // move.l d7,d0; bra.w CASE.
    poke_words(&mut e, CHAIN, &[0x2007, 0x6000, disp(CHAIN + 2, CASE)]);
    poke_words(&mut e, CASE, &case_words());
    e.poke(FORMAT, b"%Y%X%s%d ab\0");
    poke_words(&mut e, STUB, &[0x4ef9, 0x0011, 0x42f0]);
    e.volume_ptrs = vec![VOLUME_PTR; 4];
    e
}

/// A `.new` of two items: the table (first long 8 = its size, item 1 at 0xffffffff), then item
/// 0: a zero header (one row of 16 pixels, no planes) and its empty mask.
fn new_file() -> Vec<u8> {
    let mut b = vec![0, 0, 0, 8, 0xff, 0xff, 0xff, 0xff];
    b.extend([0; 12]);
    b
}

/// Every `.new` the intro loads.
const NEW_FILES: [&str; 14] = [
    "blocks.new",
    "gypsy.new",
    "intro.new",
    "intro_1.new",
    "intro_1a.new",
    "intro_1b.new",
    "intro_2.new",
    "intro_2a.new",
    "intro_3.new",
    "mainmenu.new",
    "montage.new",
    "title.new",
    "vellum1.new",
    "woods.new",
];

/// The keys sent once a script is used up, one every 20 ticks: answers, letters, Return, the
/// arrows, a song key, Escape.
const KEYS: [Key; 14] = [
    Key::Char('a'),
    Key::Char('b'),
    Key::Char('m'),
    Key::Char('f'),
    Key::Enter,
    Key::Space,
    Key::Left,
    Key::Right,
    Key::Up,
    Key::Down,
    Key::Char('1'),
    Key::Char('z'),
    Key::Backspace,
    Key::Escape,
];

/// A creation from the menu: name, Return, sex, accept the portrait, through the woods and the
/// wagon's pages, seven answers, the farewell pages, then Escape out of the introduction that
/// follows.
fn creation() -> Vec<Key> {
    let mut k = vec![Key::Char('c'), Key::Char('a'), Key::Char('b'), Key::Enter];
    k.extend([Key::Char('m'), Key::Char('c')]);
    k.extend([Key::Space; 6]);
    k.extend((0..7).map(|q| Key::Char(if q % 2 == 0 { 'a' } else { 'b' })));
    k.extend([Key::Space; 6]);
    k.push(Key::Escape);
    k
}

/// What a run must reach.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Want {
    /// main still runs at the last tick and has read every key sent.
    Waiting,
    /// main returned this.
    Ended(IntroOutcome),
}

/// What a run reached: main's outcome (`None`: still running), the keys not read yet, and the
/// stubs hit but the clock's (a passage case the evaluator could not step through hits one).
type Reached = (Option<IntroOutcome>, usize, Vec<&'static str>);

/// One run of main on its own thread: `opening` ticks of the opening, then Escape and `script`
/// one key every 150 ticks, then [`KEYS`] round and round until 40 ticks before `ticks`;
/// `plain` swaps u's key table for a working one.
#[allow(clippy::disallowed_methods)] // the threads only bound the time the runs may take
fn spawn(
    words: [i16; 2],
    plain: bool,
    opening: u32,
    script: Vec<Key>,
    ticks: u32,
) -> Receiver<Reached> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let font = u6_formats::lzw::encode(&font_bytes()).unwrap();
        let new = u6_formats::lzw::encode(&new_file()).unwrap();
        let mut files: std::collections::BTreeMap<String, Vec<u8>> = NEW_FILES
            .iter()
            .map(|n| (n.to_string(), new.clone()))
            .collect();
        files.insert("u6.set".into(), font);
        let objlist = vec![0; u6_formats::objlist::OBJLIST_LEN];
        files.insert("savegame/objlist".into(), objlist);
        let files = Files::new(files);
        let mut st = IntroState::new(extreme(words), files.clone());
        if plain {
            keyboard(&mut st);
        }
        let out = Rc::new(RefCell::new(None));
        let o2 = Rc::clone(&out);
        let mut e = Engine::with_game(State::empty(), files, move |_s, io| async move {
            let o = main(&mut st, &io, 0x1234, &LAUNCHER_ARGS).await;
            *o2.borrow_mut() = Some(o);
        });
        let scripted = opening + 150 * script.len() as u32;
        for t in 0..ticks {
            let key = match t.checked_sub(opening) {
                _ if t + 40 > ticks => None,
                Some(0) => Some(Key::Escape),
                Some(d) if t <= scripted && d % 150 == 0 => Some(script[(d / 150 - 1) as usize]),
                Some(_) if t > scripted && t % 20 == 0 => {
                    Some(KEYS[(t / 20) as usize % KEYS.len()])
                }
                _ => None,
            };
            let ev: Vec<InputEvent> = key.into_iter().map(InputEvent::Key).collect();
            e.tick(&ev, 20);
            if e.finished() {
                break;
            }
        }
        let stubs = e.stub_hits().into_keys();
        let stubs = stubs.filter(|k| !k.starts_with("u 0x1021bc")).collect();
        let _ = tx.send((out.take(), e.io().pending_events(), stubs));
    });
    rx
}

#[test]
fn a_u_of_extreme_words_runs_the_opening_the_menu_and_its_entries() {
    let (min, max) = (i16::MIN, i16::MAX);
    let played = |created| Want::Ended(IntroOutcome::Play { created });
    let mut runs = Vec::new();
    for words in [[min, min], [max, max], [min, max], [max, min]] {
        let mut add = |name: &str, plain, opening, script: Vec<Key>, ticks, want| {
            let what = format!("words {words:04x?}, {name}");
            runs.push((what, want, spawn(words, plain, opening, script, ticks)));
        };
        add("the opening", true, 3000, Vec::new(), 3000, Want::Waiting);
        add("a creation", true, 100, creation(), 6000, played(true));
        let (i, t, a, j) = (
            Key::Char('i'),
            Key::Char('t'),
            Key::Char('a'),
            Key::Char('j'),
        );
        add(
            "the introduction",
            true,
            100,
            vec![i, Key::Escape, j],
            2500,
            played(false),
        );
        add(
            "the transfer",
            true,
            100,
            vec![t, Key::Escape, j],
            2500,
            played(false),
        );
        let mut credits = vec![a];
        credits.extend([Key::Space; 8]);
        credits.push(j);
        add(
            "the acknowledgments",
            true,
            100,
            credits,
            3500,
            played(false),
        );
        add("journey onward", true, 100, vec![j], 1000, played(false));
        add(
            "u's own key table",
            false,
            100,
            creation(),
            2500,
            Want::Waiting,
        );
    }
    // The runs go on side by side; each gets 300 s from when it is waited for.
    let failed: Vec<String> = runs
        .into_iter()
        .filter_map(|(what, want, rx)| {
            let why = match rx.recv_timeout(Duration::from_secs(300)) {
                Ok((Some(o), _, s)) if s.is_empty() && want == Want::Ended(o.clone()) => {
                    return None;
                }
                Ok((None, 0, s)) if s.is_empty() && want == Want::Waiting => return None,
                Ok((got, unread, s)) => format!("reached {got:?}, {unread} keys unread, {s:?}"),
                Err(RecvTimeoutError::Timeout) => "still running after 300 s".into(),
                Err(RecvTimeoutError::Disconnected) => "panicked".into(),
            };
            Some(format!("{what}: {why}"))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

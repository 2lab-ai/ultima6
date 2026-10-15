//! A foreign `u` of extreme words, run through main in a debug build (every sum overflow-checked).
//!
//! What the image holds: every word i16::MIN or i16::MAX (four patterns: all MIN, all MAX, and
//! the two alternations), but for main's argument string; a compare chain at 0x10ac7a whose
//! one case (every passage) stores the scroll frame (blocks item 1 at 0x10, 0x10), lays the
//! passage box and its screen place out of MIN, MAX and the volume word A4+0x8d2, fills the
//! box and prints `%Y%X%s%d` with the volume word, the first volume-name pointer (set to
//! 0xfffffff0) and −1; song file names `song0` .. `song12`; and, for the runs that type, a
//! working key table. The introduction flag (A4) starts clear.
//!
//! What the files hold: the synthetic font, a zeroed `savegame/objlist`, and for every `.new`
//! the intro loads one 20-byte file whose table gives item 0 the offset 8 (an empty item) and
//! item 1 the offset 0xffffffff. Higher indices are past the table, except where index × 4
//! wraps as a word (mem.rs `item_at`): 0x7fff (−4) reads its offset from the 4 bytes before
//! the table, 0x8000 (0) aliases item 0.
//!
//! What each run must reach (main's outcome, the marks it leaves, the songs it asks for):
//!
//! - the opening, stepped by Space every 150 ticks: its song (2), then the menu's (1), and
//!   main still running with every key read;
//! - a creation: through the woods, the wagon and the farewell to the objlist (`Play {
//!   created: true }`), with its song (4), its portrait frame and its vellum view;
//! - the introduction, the transfer (Escape at the disk question) and the acknowledgments,
//!   each from the menu (song 1) and back to it, then Journey Onward (`Play { created: false
//!   }`), each with its own mark: the introduction flag and song 3; the portrait frame, the
//!   vellum view and song 4; the vellum view alone;
//! - Journey Onward at once, with no mark;
//! - with u's own key table: the opening's song, and main still running with every key read.
//!
//! No run may hit a stub but the clock's, so every passage steps the case to its end.
//!
//! Not covered: the passage cases of a real `u`, the save readers of the transfer (there is no
//! save), the mouse, and items with pictures (the `.new` holds none).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::intro::exe::UExe;
use crate::intro::mem::View;
use crate::intro::{IntroOutcome, IntroState, LAUNCHER_ARGS, main};
use crate::sound::MedCall;
use crate::state::State;

use super::asm::*;
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
/// The print routine the stub jumps to.
const PRINT: u32 = 0x0011_42f0;
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
    w.extend([MOVE_W_IMM_TO_A4, 0x10, 0x2cfc]);
    w.extend([MOVE_W_IMM_TO_A4, 0x10, 0x2cfe]);
    w.extend([MOVE_W_IMM_TO_A4, 1, 0x2d00]);
    // The box and its place: the volume word, MIN and MAX.
    w.extend([MOVE_W_A4_TO_D0, 0x08d2, MOVE_W_D0_TO_A4, 0x2d0a]);
    w.extend([MOVE_W_IMM_TO_A4, min, 0x2d0c, MOVE_W_D0_TO_A4, 0x2d0e]);
    w.extend([MOVE_W_IMM_TO_A4, max, 0x2d10, MOVE_W_D0_TO_A4, 0x2d1c]);
    w.extend([MOVE_W_IMM_TO_A4, min, 0x2d1e]);
    // moveq #5,d0; lea $2d02(a4),a0; jsr fill.
    w.extend([moveq_d0(5), LEA_A4_A0, 0x2d02, JSR_PC]);
    w.push(disp(CASE + 2 * w.len() as u32 - 2, FILL_VIEW));
    // −1 (%d); the volume-name pointer of entry 0 (%s); the volume word twice (%X, %Y).
    w.extend([moveq_d0(-1), PUSH_W_D0, SUBQ_W_2_A7, SUBQ_W_2_A7]);
    w.extend([LEA_A4_A0, 0x08ac, moveq_d0(0)]);
    w.extend(MOVE_L_A0_D0_TO_SP);
    w.extend([MOVE_W_A4_TO_D0, 0x08d2, PUSH_W_D0, PUSH_W_D0, PEA_PC]);
    w.push(disp(CASE + 2 * w.len() as u32 - 2, FORMAT));
    w.push(JSR_PC);
    w.push(disp(CASE + 2 * w.len() as u32 - 2, STUB));
    // lea 14(a7),a7; bra.w (to the tail).
    w.extend([LEA_A7_A7, 14, BRA_W, 0]);
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
    poke_words(&mut e, CHAIN, &[MOVE_L_D7_D0, BRA_W, disp(CHAIN + 2, CASE)]);
    poke_words(&mut e, CASE, &case_words());
    e.poke(FORMAT, b"%Y%X%s%d ab\0");
    poke_words(&mut e, STUB, &jmp_abs(PRINT));
    e.volume_ptrs = vec![VOLUME_PTR; 4];
    // Song files named by number, so the run's log says which songs main asked for.
    e.song_names = (0..13).map(|n| format!("song{n}").into_bytes()).collect();
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

/// `keys` one every 150 ticks from tick `at`.
fn paced(at: u32, keys: &[Key]) -> Vec<(u32, Key)> {
    let at = (0..).map(|i| at + 150 * i);
    at.zip(keys.iter().copied()).collect()
}

/// From the menu: Escape at tick 100 ends the opening, then `keys` one every 150 ticks.
fn from_menu(keys: &[Key]) -> Vec<(u32, Key)> {
    let mut k = vec![(100, Key::Escape)];
    k.extend(paced(250, keys));
    k
}

/// After `keys`, [`KEYS`] round and round, one every 20 ticks up to tick `until`.
fn then_fuzz(mut keys: Vec<(u32, Key)>, until: u32) -> Vec<(u32, Key)> {
    let from = keys.last().map_or(0, |&(t, _)| t) + 20;
    let fuzz = (from..until).step_by(20).zip(KEYS.iter().copied().cycle());
    keys.extend(fuzz);
    keys
}

/// The state main left behind that tells which menu entries ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Marks {
    /// A4+0 set: main ran the introduction from the menu.
    intro_seen: bool,
    /// The portrait frame on the screen (A4+0x2c64) laid out: the transfer or the creation.
    portrait_frame: bool,
    /// The vellum view (A4+0x17f4) set to the menu panel: the acknowledgments, the transfer or
    /// the creation.
    vellum_view: bool,
}

impl Marks {
    fn of(st: &IntroState) -> Self {
        Self {
            intro_seen: st.intro_seen != 0,
            portrait_frame: st.views.v2c64 != View::default(),
            vellum_view: st.views.v17f4 != View::default(),
        }
    }
}

/// What a run reached.
#[derive(Debug)]
struct Reached {
    /// How main ended (`None`: still running at the last tick).
    outcome: Option<IntroOutcome>,
    /// What main left behind (all false while it runs).
    marks: Marks,
    /// The song files main asked for, in order.
    songs: Vec<String>,
    /// Keys sent and not read yet.
    unread: usize,
    /// Stubs hit but the clock's (a passage case the evaluator could not step through hits
    /// one).
    stubs: Vec<&'static str>,
}

/// What a run must reach: main's outcome (`None`: still running, every key read), the marks,
/// and songs main must have asked for in this order.
struct Want {
    outcome: Option<IntroOutcome>,
    marks: Marks,
    songs: &'static [&'static str],
}

impl Want {
    fn met(&self, r: &Reached) -> bool {
        let mut songs = r.songs.iter();
        let in_order = self.songs.iter().all(|s| songs.any(|g| g == s));
        r.stubs.is_empty()
            && r.outcome == self.outcome
            && (self.outcome.is_some() || r.unread == 0)
            && r.marks == self.marks
            && in_order
    }
}

/// One run of main on its own thread for `ticks` ticks, `keys` sent at their ticks (none in
/// the last 40); `plain` swaps u's key table for a working one.
#[allow(clippy::disallowed_methods)] // the threads only bound the time the runs may take
fn spawn(words: [i16; 2], plain: bool, keys: Vec<(u32, Key)>, ticks: u32) -> Receiver<Reached> {
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
        // The introduction flag (the word at A4) starts clear, so the marks can tell whether the
        // introduction ran.
        st.intro_seen = 0;
        if plain {
            keyboard(&mut st);
        }
        let calls = st.music.handle();
        let out = Rc::new(RefCell::new(None));
        let o2 = Rc::clone(&out);
        let mut e = Engine::with_game(State::empty(), files, move |_s, io| async move {
            let o = main(&mut st, &io, 0x1234, &LAUNCHER_ARGS).await;
            *o2.borrow_mut() = Some((o, Marks::of(&st)));
        });
        let mut songs = Vec::new();
        let mut keys = keys.into_iter().peekable();
        for t in 0..ticks {
            let mut ev = Vec::new();
            while let Some((_, k)) = keys.next_if(|&(at, _)| at == t && t + 40 <= ticks) {
                ev.push(InputEvent::Key(k));
            }
            e.tick(&ev, 20);
            songs.extend(calls.take().into_iter().filter_map(|c| match c {
                MedCall::Load(name) => Some(name),
                _ => None,
            }));
            if e.finished() {
                break;
            }
        }
        let (outcome, marks) = out
            .take()
            .map_or((None, Marks::default()), |(o, m)| (Some(o), m));
        let stubs = e.stub_hits().into_keys();
        let stubs = stubs.filter(|k| !k.starts_with("u 0x1021bc")).collect();
        let unread = e.io().pending_events();
        let _ = tx.send(Reached {
            outcome,
            marks,
            songs,
            unread,
            stubs,
        });
    });
    rx
}

#[test]
fn a_u_of_extreme_words_runs_the_opening_the_menu_and_its_entries() {
    let (min, max) = (i16::MIN, i16::MAX);
    let played = |created| Some(IntroOutcome::Play { created });
    let marks = |intro_seen, portrait_frame, vellum_view| Marks {
        intro_seen,
        portrait_frame,
        vellum_view,
    };
    let none = Marks::default();
    let (i, t, a, j) = (
        Key::Char('i'),
        Key::Char('t'),
        Key::Char('a'),
        Key::Char('j'),
    );
    let mut credits = vec![a];
    credits.extend([Key::Space; 8]);
    credits.push(j);
    // Space every 150 ticks steps the opening's waits; at the menu Space does nothing.
    let spaces = paced(150, &[Key::Space; 60]);
    let mut runs = Vec::new();
    for words in [[min, min], [max, max], [min, max], [max, min]] {
        let mut add = |name: &str, plain, keys, ticks, want: Want| {
            let what = format!("words {words:04x?}, {name}");
            runs.push((what, want, spawn(words, plain, keys, ticks)));
        };
        let opening = &["song2", "song1"];
        add(
            "the opening to the menu",
            true,
            spaces.clone(),
            9500,
            Want {
                outcome: None,
                marks: none,
                songs: opening,
            },
        );
        add(
            "a creation",
            true,
            from_menu(&creation()),
            6000,
            Want {
                outcome: played(true),
                marks: marks(false, true, true),
                songs: &["song4"],
            },
        );
        add(
            "the introduction",
            true,
            from_menu(&[i, Key::Escape, j]),
            2500,
            Want {
                outcome: played(false),
                marks: marks(true, false, false),
                songs: &["song1", "song3"],
            },
        );
        add(
            "the transfer",
            true,
            from_menu(&[t, Key::Escape, j]),
            2500,
            Want {
                outcome: played(false),
                marks: marks(false, true, true),
                songs: &["song1", "song4"],
            },
        );
        add(
            "the acknowledgments",
            true,
            from_menu(&credits),
            3500,
            Want {
                outcome: played(false),
                marks: marks(false, false, true),
                songs: &["song1"],
            },
        );
        add(
            "journey onward",
            true,
            from_menu(&[j]),
            1000,
            Want {
                outcome: played(false),
                marks: none,
                songs: &["song1"],
            },
        );
        let keys = then_fuzz(from_menu(&creation()), 2460);
        add(
            "u's own key table",
            false,
            keys,
            2500,
            Want {
                outcome: None,
                marks: none,
                songs: &["song2"],
            },
        );
    }
    // The runs go on side by side; each gets 300 s from when it is waited for.
    let failed: Vec<String> = runs
        .into_iter()
        .filter_map(|(what, want, rx)| {
            let why = match rx.recv_timeout(Duration::from_secs(300)) {
                Ok(r) if want.met(&r) => return None,
                Ok(r) => format!("reached {r:?}"),
                Err(RecvTimeoutError::Timeout) => "still running after 300 s".into(),
                Err(RecvTimeoutError::Disconnected) => "panicked".into(),
            };
            Some(format!("{what}: {why}"))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

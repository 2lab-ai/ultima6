//! A foreign `u` whose every word (tables, strings, scripts and code alike) is i16::MIN or
//! i16::MAX: main runs the opening, the menu and each menu entry for a bounded number of ticks
//! without a panic, and a creation through the wagon to the objlist. Only main's argument
//! string is put back, and the runs that type swap in a working key table. In a debug build
//! every sum is overflow-checked, so a sum the port makes wider than the 68000's word shows
//! here.

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

/// The image with every word `words[0]`, `words[1]`, … and main's argument string in place.
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
    e
}

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
/// wagon's pages, seven answers, the farewell pages.
fn creation() -> Vec<Key> {
    let mut k = vec![Key::Char('c'), Key::Char('a'), Key::Char('b'), Key::Enter];
    k.extend([Key::Char('m'), Key::Char('c')]);
    k.extend([Key::Space; 6]);
    k.extend((0..7).map(|q| Key::Char(if q % 2 == 0 { 'a' } else { 'b' })));
    k.extend([Key::Space; 6]);
    k
}

/// One run of main on its own thread: `opening` ticks of the opening, then Escape and `script`
/// one key every 150 ticks, then [`KEYS`] round and round until `ticks`; `plain` swaps u's key
/// table for a working one. The receiver gets how main ended (`None`: still running at
/// `ticks`).
#[allow(clippy::disallowed_methods)] // the threads only bound the time the runs may take
fn spawn(
    words: [i16; 2],
    plain: bool,
    opening: u32,
    script: Vec<Key>,
    ticks: u32,
) -> Receiver<Option<IntroOutcome>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let font = u6_formats::lzw::encode(&font_bytes()).unwrap();
        let objlist = vec![0; u6_formats::objlist::OBJLIST_LEN];
        let files = Files::new(
            [
                ("u6.set".to_string(), font),
                ("savegame/objlist".to_string(), objlist),
            ]
            .into(),
        );
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
        let _ = tx.send(out.take());
    });
    rx
}

#[test]
fn a_u_of_extreme_words_runs_the_opening_and_the_menu_without_a_panic() {
    let (min, max) = (i16::MIN, i16::MAX);
    let created = Some(IntroOutcome::Play { created: true });
    let mut runs = Vec::new();
    for words in [[min, min], [max, max], [min, max], [max, min]] {
        let mut add = |name: &str, plain, opening, script: Vec<Key>, ticks, want| {
            let what = format!("words {words:04x?}, {name}");
            runs.push((what, want, spawn(words, plain, opening, script, ticks)));
        };
        add("the opening", true, 3000, Vec::new(), 3000, None);
        add(
            "a creation",
            true,
            100,
            creation(),
            6000,
            Some(created.clone()),
        );
        for entry in ['i', 't', 'a', 'j'] {
            let name = format!("menu key {entry}");
            add(&name, true, 100, vec![Key::Char(entry)], 2500, None);
        }
        add("u's own key table", false, 100, creation(), 2500, None);
    }
    // The runs go on side by side; each gets 300 s from when it is waited for.
    let failed: Vec<String> = runs
        .into_iter()
        .filter_map(|(what, want, rx)| {
            let why = match rx.recv_timeout(Duration::from_secs(300)) {
                Ok(got) if want.as_ref().is_none_or(|w| *w == got) => return None,
                Ok(got) => format!("ended {got:?}"),
                Err(RecvTimeoutError::Timeout) => "still running after 300 s".into(),
                Err(RecvTimeoutError::Disconnected) => "panicked".into(),
            };
            Some(format!("{what}: {why}"))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

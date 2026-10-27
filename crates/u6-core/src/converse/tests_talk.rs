//! Spec 22 (conversation by click) on hand-built scripts and a synthetic font (glyph `c` = eight
//! rows of the byte `c`): the Job and Name buttons below the portrait exist only while the
//! conversation reads the player's words after "you say" (op f7); a press on one, or on a keyword
//! the scroll shows in the alert colour, is typing its word and Enter: the same state and frame
//! as the keys. All text here is the test's own (made-up words).

use std::collections::BTreeMap;
use std::rc::Rc;

use u6_formats::font::BitFont;

use super::begin;
use super::disasm::catalog::ScriptUnits;
use super::tests::state;
use super::tests_locale::{pack_for, screen, script, talk_files};
use crate::engine::{Engine, InputEvent, Key, POINTER_LEFT};
use crate::files::Files;
use crate::locale::{self, Pack};
use crate::state::State;
use crate::ui::Language;
use crate::ui::talk::{self, Target};
use crate::ui::text::CELL;

/// Two keyword blocks, `job` then `name`, after the player's words; no match prints `No.`.
const BODY: &[u8] = b"\xf7\xefjob\xf6Work.\xb6\xefname\xf6Me $Z.\xb6\xeeNo.\xb6";
/// A greeting with the keyword `@cat`, then the player's words and its block.
const KEYED: &[u8] = b"Hi @cat. Go.\xf7\xefcat\xf6Purr.\xb6\xeeNo.\xb6";
/// The synthetic ui's paper and command colours (`test_ui`).
const PAPER: u8 = 2;
const COMMAND: u8 = 5;
/// The buttons' screen row (the Status window's last, main's windows).
const ROW: usize = 12;

/// The synthetic font: every row of glyph `c` is `c`.
fn font() -> BitFont {
    BitFont {
        glyphs: (0..=255u8).map(|c| [c; 8]).collect(),
    }
}

/// The conversation state: main's windows, a font, the keyword colour on, and `pack` (or none).
fn talk_state(body: &[u8], pack: Option<&Pack>) -> (State, Files) {
    let f = talk_files(&script(body));
    let mut s = state(&[]);
    s.ui.font = Some(font());
    let t = s.ui.tables.clone();
    s.ui.text.main_windows(&t);
    s.ui.text.keyword_colour = true;
    if let Some(p) = pack {
        locale::install(&mut s, &f, Rc::new(p.clone()), None).expect("the pack checks");
    }
    (s, f)
}

/// The conversation with NPC 3 in `s`, run idle until it waits for the player.
fn waiting(s: State, f: Files) -> Engine {
    let mut e = Engine::with_game(s, f, |st, io| async move {
        begin(&super::Cx::new(&st, &io), 0, 3).await;
    });
    for _ in 0..12 {
        e.tick(&[], 16);
    }
    assert!(!e.finished(), "waiting for the player");
    e
}

/// Ticks `e` idle until the conversation ends.
fn finish(e: &mut Engine) {
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished(), "the conversation ended");
}

/// The pixels of the buttons' row in the command colour.
fn button_pixels(e: &Engine) -> usize {
    let fb = e.frame();
    (ROW * CELL..(ROW + 1) * CELL)
        .flat_map(|y| (176..312).map(move |x| (x, y)))
        .filter(|&(x, y)| fb.get(x, y) == Some(COMMAND))
        .count()
}

/// Whether every pixel of the buttons' row is the paper colour.
fn row_blank(e: &Engine) -> bool {
    let fb = e.frame();
    (ROW * CELL..(ROW + 1) * CELL).all(|y| (176..312).all(|x| fb.get(x, y) == Some(PAPER)))
}

/// The middle pixel of button `i`.
fn button_pixel(e: &Engine, i: usize) -> (i16, i16) {
    let b = &e.state().ui.talk.buttons[i];
    let col = usize::from(b.col) + b.cells.len() / 2;
    ((col * CELL + 4) as i16, (ROW * CELL + 4) as i16)
}

fn at(x: i16, y: i16, buttons: u8) -> InputEvent {
    InputEvent::Pointer { x, y, buttons }
}

/// `text` as the page sends it: a Hangul syllable as text, a line feed as Enter, a space as
/// Space, anything else as its key.
fn typed(text: &str) -> Vec<InputEvent> {
    text.chars()
        .map(|c| match c {
            '\n' => InputEvent::Key(Key::Enter),
            ' ' => InputEvent::Key(Key::Space),
            '\u{ac00}'..='\u{d7a3}' => InputEvent::Text(c),
            c => InputEvent::Key(Key::Char(c)),
        })
        .collect()
}

/// The two runs of a conversation from `s`: the pointer moved to (`x`, `y`) and pressed there,
/// against the pointer moved there and `line` typed in one tick; both to their end. The press
/// run's engine, and whether the two ended in the same state and frame.
fn press_against_typing(
    s: &State,
    f: &Files,
    xy: impl Fn(&Engine) -> (i16, i16),
    line: &str,
) -> (Engine, bool) {
    let mut pressed = waiting(s.clone(), f.clone());
    let (x, y) = xy(&pressed);
    pressed.tick(&[at(x, y, 0)], 16);
    pressed.tick(&[at(x, y, POINTER_LEFT), at(x, y, 0)], 16);
    finish(&mut pressed);
    let mut keys = waiting(s.clone(), f.clone());
    keys.tick(&[at(x, y, 0)], 16);
    keys.tick(&typed(line), 16);
    finish(&mut keys);
    let same =
        *pressed.state() == *keys.state() && pressed.frame().pixels() == keys.frame().pixels();
    (pressed, same)
}

/// The pack of `body`'s runs (each as itself) with the keyword aliases `aliases` (keyword, words)
/// of NPC 3's script.
fn aliased(body: &[u8], aliases: &[(&str, &[&str])]) -> Pack {
    let script = script(body);
    let units = ScriptUnits::build(b'a', 3, &script);
    let lines: BTreeMap<&str, &str> = units
        .runs
        .iter()
        .map(|r| (r.unit.text.as_str(), r.unit.text.as_str()))
        .collect();
    pack_for(&script, Language::Korean, &lines)
        .with_aliases(aliases.iter().map(|(k, words)| {
            (
                (b'a', 3),
                k.as_bytes().to_vec(),
                words.iter().map(|w| w.as_bytes().to_vec()).collect(),
            )
        }))
        .unwrap()
}

#[test]
fn the_buttons_show_only_while_the_words_after_you_say_are_read() {
    // `job`'s answer reads a string (f9) before the conversation ends.
    let body: &[u8] = b"\xf7\xefjob\xf6Work.\xf9\x05\xb3$5\xb6\xeeNo.\xb6";
    let (s, f) = talk_state(body, None);
    let mut e = waiting(s, f);
    let st = e.state().clone();
    assert_eq!(st.ui.talk.buttons.len(), 2, "two buttons in the f7 read");
    let words: Vec<&[u8]> = st.ui.talk.buttons.iter().map(|b| &b.word[..]).collect();
    assert_eq!(words, [&b"job"[..], b"name"], "the script's keywords");
    let drawn = button_pixels(&e);
    assert!(drawn > 0 && !row_blank(&e), "drawn in the command colour");
    println!(
        "f7 read: buttons {}, pixels {drawn}",
        st.ui.talk.buttons.len()
    );
    e.tick(&typed("job\n"), 16);
    for _ in 0..12 {
        e.tick(&[], 16);
    }
    assert!(
        !e.finished() && e.state().ui.input.line_input,
        "waiting at the f9 read"
    );
    assert!(screen(&e.state()).contains("Work."), "the answer of `job`");
    assert!(
        e.state().ui.talk.buttons.is_empty(),
        "no button after the f7 read, none in f9"
    );
    assert!(row_blank(&e), "the row is the paper again");
    // A press and release where Name was types nothing in f9; nor in fb.
    let quiet = |e: &mut Engine| {
        let before = e.state().converse.input.clone();
        let (x, y) = ((33 * CELL + 4) as i16, (ROW * CELL + 4) as i16);
        e.tick(&[at(x, y, POINTER_LEFT), at(x, y, 0)], 16);
        for _ in 0..8 {
            e.tick(&[], 16);
        }
        assert!(
            !e.finished() && e.state().converse.input == before,
            "nothing typed"
        );
    };
    quiet(&mut e);
    e.tick(&typed("typed\n"), 16);
    finish(&mut e);
    let (s, f) = talk_state(b"\xfb\x04\xb2#4\xb6", None);
    let mut e = waiting(s, f);
    assert!(e.state().ui.talk.buttons.is_empty(), "no button in fb");
    assert_eq!(button_pixels(&e), 0, "nothing drawn");
    quiet(&mut e);
    e.tick(&typed("42\n"), 16);
    finish(&mut e);
    println!("f9, fb reads: no buttons, a press types nothing: pass");
}

#[test]
fn a_press_on_job_or_name_is_typing_the_word_and_enter() {
    let (s, f) = talk_state(BODY, None);
    for (i, line, answer) in [(0, "job\n", "Work."), (1, "name\n", "Me name.")] {
        let (e, same) = press_against_typing(&s, &f, |e| button_pixel(e, i), line);
        assert!(same, "button {i}: the state and frame of typing it");
        assert_eq!(e.state().converse.input, line.trim_end().as_bytes());
        assert!(
            screen(&e.state()).ends_with(answer),
            "button {i}: its answer"
        );
    }
    println!("Job and Name presses equal typing: pass");
}

#[test]
fn with_a_pack_the_name_button_types_its_first_alias() {
    let pack = aliased(BODY, &[("job", &["일"]), ("name", &["이름", "성명"])]);
    let (s, f) = talk_state(BODY, Some(&pack));
    let e = waiting(s.clone(), f.clone());
    let words: Vec<Vec<u8>> = e
        .state()
        .ui
        .talk
        .buttons
        .iter()
        .map(|b| b.word.clone())
        .collect();
    assert_eq!(
        words,
        ["일".as_bytes().to_vec(), "이름".as_bytes().to_vec()]
    );
    let cells: Vec<usize> = e
        .state()
        .ui
        .talk
        .buttons
        .iter()
        .map(|b| b.cells.len())
        .collect();
    assert_eq!(cells, [3, 4], "one cell a syllable, a blank each side");
    let (e, same) = press_against_typing(&s, &f, |e| button_pixel(e, 1), "이름\n");
    assert!(same, "the state and frame of typing the alias");
    assert_eq!(e.state().converse.input, "이름".as_bytes());
    assert!(
        screen(&e.state()).ends_with("Me 이름."),
        "the answer of `name`"
    );
    println!("pack alias press equals typing it: pass");
}

#[test]
fn a_press_on_a_keyword_types_it_and_a_press_beside_it_nothing() {
    let (s, f) = talk_state(KEYED, None);
    let e = waiting(s.clone(), f.clone());
    let st = e.state().clone();
    let spans = &st.ui.text.keys.0;
    assert_eq!(spans.len(), 1, "one keyword shown");
    assert_eq!(spans[0].word(), b"cat");
    let w = st.ui.text.window(crate::ui::WindowId::Messages);
    let (sx, sy) = (
        usize::from(w.x) + usize::from(spans[0].col),
        usize::from(w.y) + usize::from(spans[0].row),
    );
    let pixel = |col: usize| ((col * CELL + 4) as i16, (sy * CELL + 4) as i16);
    for c in 0..3 {
        let (x, y) = pixel(sx + c);
        assert!(
            matches!(talk::target_at(&st, x, y), Some(Target::Key { .. })),
            "cell {c} of the keyword"
        );
    }
    for col in [sx - 1, sx + 3] {
        let (x, y) = pixel(col);
        assert_eq!(talk::target_at(&st, x, y), None, "beside the keyword");
    }
    let (e, same) = press_against_typing(&s, &f, |_| pixel(sx + 1), "cat\n");
    assert!(same, "the state and frame of typing the keyword");
    assert_eq!(e.state().converse.input, b"cat");
    assert!(screen(&e.state()).ends_with("Purr."), "its answer");
    // Beside it: the read goes on.
    let mut e = waiting(s, f);
    let (x, y) = pixel(sx + 3);
    e.tick(&[at(x, y, POINTER_LEFT), at(x, y, 0)], 16);
    for _ in 0..8 {
        e.tick(&[], 16);
    }
    assert!(
        !e.finished() && e.state().converse.input.is_empty(),
        "nothing typed"
    );
    println!("keyword press equals typing it, beside it nothing: pass");
}

#[test]
fn a_braced_keyword_with_a_space_types_both_words() {
    let body: &[u8] = b"See @{big cat}.\xf7\xefbig cat\xf6Roar.\xb6\xeeNo.\xb6";
    let (mut s, f) = talk_state(body, None);
    s.ui.text.set_wide(true);
    let e = waiting(s.clone(), f.clone());
    let st = e.state().clone();
    let words: Vec<Vec<u8>> = st.ui.text.keys.0.iter().map(|k| k.word()).collect();
    assert_eq!(words, [b"big cat".to_vec()], "the braced text, one span");
    let k = &st.ui.text.keys.0[0];
    let w = st.ui.text.window(crate::ui::WindowId::Messages);
    let col = usize::from(w.x) + usize::from(k.col) + 5;
    let row = usize::from(w.y) + usize::from(k.row);
    let xy = ((col * CELL + 4) as i16, (row * CELL + 4) as i16);
    let (e, same) = press_against_typing(&s, &f, |_| xy, "big cat\n");
    assert!(same, "the state and frame of typing it");
    assert_eq!(e.state().converse.input, b"big cat");
    println!("braced keyword press equals typing it: pass");
}

#[test]
fn the_hover_inverts_a_button_and_a_keyword_and_puts_them_back() {
    let (s, f) = talk_state(KEYED, None);
    let mut e = waiting(s, f);
    let k = e.state().ui.text.keys.0[0].clone();
    let w = *e.state().ui.text.window(crate::ui::WindowId::Messages);
    let (col, row) = (usize::from(w.x + k.col), usize::from(w.y + k.row));
    // The buttons' row and the keyword's cells, as drawn.
    let cells = |e: &Engine| {
        let fb = e.frame();
        let key = (row * CELL..(row + 1) * CELL)
            .flat_map(|y| (col * CELL..(col + 3) * CELL).map(move |x| (x, y)));
        let buttons = (ROW * CELL..(ROW + 1) * CELL).flat_map(|y| (176..312).map(move |x| (x, y)));
        key.chain(buttons)
            .map(|(x, y)| fb.get(x, y))
            .collect::<Vec<_>>()
    };
    let normal = cells(&e);
    let (bx, by) = button_pixel(&e, 0);
    e.tick(&[at(bx, by, 0)], 16);
    assert_eq!(e.state().ui.talk.hot, Some(Target::Button(0)));
    let b = e.state().ui.talk.buttons[0].clone();
    let x0 = usize::from(b.col) * CELL;
    let fb = e.frame();
    let left = (x0..x0 + b.cells.len() * CELL).filter(|&x| x % CELL == 0);
    let inverse = left.clone().all(|x| fb.get(x, ROW * CELL) == Some(PAPER));
    assert!(
        inverse,
        "the key cap inverse: its cells' paper the paper colour"
    );
    // Off every target: the frame as before.
    e.tick(&[at(4, 4, 0)], 16);
    assert_eq!(e.state().ui.talk.hot, None);
    assert!(cells(&e) == normal, "put back");
    e.tick(
        &[at((col * CELL + 4) as i16, (row * CELL + 4) as i16, 0)],
        16,
    );
    let alert = e.state().ui.tables.colours.alert;
    let fb = e.frame();
    let mut left = (col * CELL..(col + 3) * CELL).filter(|&x| x % CELL == 0);
    let inverse = left.all(|x| fb.get(x, row * CELL) == Some(alert));
    assert!(inverse, "the keyword inverse: its paper the alert colour");
    e.tick(&[at(4, 4, 0)], 16);
    assert!(cells(&e) == normal, "put back");
    println!("hover inverts and restores: pass");
}

#[test]
fn a_keyword_moves_up_with_the_scroll_and_is_gone_once_it_left() {
    let (s, _) = talk_state(BODY, None);
    let after = |lines: usize| {
        let mut e = Engine::with_game(s.clone(), Files::default(), move |st, io| async move {
            crate::ui::printf::printf(&st, &io, b"x\nHi @cat.\n", &[]).await;
            for _ in 0..lines {
                crate::ui::printf::printf(&st, &io, b"x\n", &[]).await;
            }
            crate::ui::print::put_raw(&st, &io, -1).await;
        });
        finish(&mut e);
        let st = e.state().clone();
        let w = *st.ui.text.window(crate::ui::WindowId::Messages);
        let hit = |row: u8| {
            let (x, y) = (w.x + 3, w.y + row);
            talk::target_at(
                &st,
                (usize::from(x) * CELL) as i16,
                (usize::from(y) * CELL) as i16,
            )
        };
        (st.ui.text.keys.0.clone(), hit(0), hit(1))
    };
    let (keys, _, on) = after(0);
    assert_eq!(
        (keys.len(), keys[0].row, on.is_some()),
        (1, 1, true),
        "on its row"
    );
    let (keys, on, below) = after(8);
    assert_eq!(keys.len(), 1, "kept by the first scroll");
    assert_eq!(
        (keys[0].row, on.is_some(), below),
        (0, true, None),
        "a row up"
    );
    let (keys, on, _) = after(9);
    assert!(keys.is_empty() && on.is_none(), "gone with its row");
    println!("keyword spans follow the scroll: pass");
}

#[test]
fn no_button_over_a_row_the_panel_does_not_leave_blank() {
    let (s, _) = talk_state(BODY, None);
    let made = |paint: bool| {
        let mut e = Engine::with_game(s.clone(), Files::default(), move |st, io| async move {
            io.draw(|fb| {
                for y in ROW * CELL..(ROW + 1) * CELL {
                    for x in 176..312 {
                        fb.set(x, y, PAPER);
                    }
                }
                if paint {
                    fb.set(200, ROW * CELL + 3, 7);
                }
            });
            talk::begin(&mut st.borrow_mut(), &io);
        });
        finish(&mut e);
        e.state().ui.talk.buttons.len()
    };
    assert_eq!(made(false), 2, "a blank row: two buttons");
    assert_eq!(made(true), 0, "one pixel not the paper: none");
    println!("buttons only over a blank row: pass");
}

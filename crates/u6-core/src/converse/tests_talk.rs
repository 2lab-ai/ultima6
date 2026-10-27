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
use crate::ui::talk::{self, Target};
use crate::ui::text::CELL;
use crate::ui::{Language, WindowId};

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

/// The conversation state: main's windows, a font, the keyword colour on, Backspace mapped to the
/// line editor's code 0x10e (as the user's keymap maps it), and `pack` (or none).
fn talk_state(body: &[u8], pack: Option<&Pack>) -> (State, Files) {
    let f = talk_files(&script(body));
    let mut s = state(&[]);
    s.ui.tables.keymap[0x41] = [0x10e, 0x10e, 0, 0];
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
    waiting_after(s, f, &[])
}

/// [`waiting`], `first` queued at its first tick.
fn waiting_after(s: State, f: Files, first: &[InputEvent]) -> Engine {
    let mut e = Engine::with_game(s, f, |st, io| async move {
        begin(&super::Cx::new(&st, &io), 0, 3).await;
    });
    e.tick(first, 16);
    for _ in 1..12 {
        e.tick(&[], 16);
    }
    assert!(!e.finished(), "waiting for the player");
    e
}

/// Ticks `e` idle `n` times, then checks the read goes on.
fn idle(e: &mut Engine, n: usize) {
    for _ in 0..n {
        e.tick(&[], 16);
    }
    assert!(
        !e.finished() && e.state().ui.input.line_input,
        "still reading the player's words"
    );
}

/// The middle pixel of window cell (`col`, `row`) of the message window.
fn message_pixel(s: &State, col: u8, row: u8) -> (i16, i16) {
    let w = s.ui.text.window(WindowId::Messages);
    let (x, y) = (usize::from(w.x + col), usize::from(w.y + row));
    ((x * CELL + 4) as i16, (y * CELL + 4) as i16)
}

/// A press and release at (`x`, `y`) with button bits `buttons`, in one tick.
fn click(e: &mut Engine, (x, y): (i16, i16), buttons: u8) {
    e.tick(&[at(x, y, buttons), at(x, y, 0)], 16);
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
    e.tick(&typed("typed\n"), 16);
    finish(&mut e);
    let (s, f) = talk_state(b"\xfb\x04\xb2#4\xb6", None);
    let mut e = waiting(s, f);
    assert!(e.state().ui.talk.buttons.is_empty(), "no button in fb");
    assert_eq!(button_pixels(&e), 0, "nothing drawn");
    e.tick(&typed("42\n"), 16);
    finish(&mut e);
    println!("f9, fb reads: no buttons: pass");
}

/// Spec 22 Contract 1 (gpt-c MUST 2): in the f9 and fb reads a click where the Name button is
/// in f7 and a click on a keyword the scroll shows leave the read as the same pointer moves
/// without a click leave it (state and frame), and the read then takes exactly what is typed.
#[test]
fn a_click_in_the_f9_and_fb_reads_types_nothing() {
    // `job`'s answer shows a keyword, then reads a string (f9) and prints it.
    let f9: &[u8] = b"\xf7\xefjob\xf6Work @cat.\xf9\x05\xb3$5\xb6\xeeNo.\xb6";
    // A keyword, then a number read (fb) and printed.
    let fb: &[u8] = b"See @cat.\xfb\x04\xb2#4\xb6";
    for (body, before, line, shown) in [(f9, "job\n", "abc\n", "abc"), (fb, "", "42\n", "42")] {
        let (s, f) = talk_state(body, None);
        let run = |clicks: bool| {
            let mut e = waiting(s.clone(), f.clone());
            e.tick(&typed(before), 16);
            idle(&mut e, 12);
            assert!(e.state().ui.talk.buttons.is_empty(), "not the f7 read");
            let k = e.state().ui.text.keys.spans.first().cloned();
            let k = k.expect("a keyword shown in the alert colour");
            let key = message_pixel(&e.state(), k.col + 1, k.row);
            let name = ((33 * CELL + 4) as i16, (ROW * CELL + 4) as i16);
            for xy in [name, key] {
                e.tick(&[at(xy.0, xy.1, 0)], 16);
                if clicks {
                    click(&mut e, xy, POINTER_LEFT);
                } else {
                    e.tick(&[], 16);
                }
            }
            idle(&mut e, 4);
            e
        };
        let (mut clicked, mut moved) = (run(true), run(false));
        assert!(
            *clicked.state() == *moved.state()
                && clicked.frame().pixels() == moved.frame().pixels(),
            "the clicks leave the read as the moves alone"
        );
        for e in [&mut clicked, &mut moved] {
            e.tick(&typed(line), 16);
            finish(e);
        }
        assert!(
            screen(&clicked.state()).ends_with(shown),
            "the read took what was typed, alone"
        );
        assert!(*clicked.state() == *moved.state(), "and ended the same");
    }
    println!("f9 and fb reads: a click types nothing: pass");
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
    let spans = &st.ui.text.keys.spans;
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
    let words: Vec<Vec<u8>> = st.ui.text.keys.spans.iter().map(|k| k.word()).collect();
    assert_eq!(words, [b"big cat".to_vec()], "the braced text, one span");
    let k = &st.ui.text.keys.spans[0];
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
    let k = e.state().ui.text.keys.spans[0].clone();
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
        (st.ui.text.keys.spans.clone(), hit(0), hit(1))
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

/// Spec 22 Contract 2 (gpt-a MUST 1): the buttons fit the Status window's last row: with their
/// blank cells, without them, or not at all; never a cell outside the row.
#[test]
fn the_buttons_fit_the_row_or_none_are_drawn() {
    // `job` aliased to a word of eight syllables (eight cells).
    let pack = aliased(BODY, &[("job", &["가나다라마바사아"])]);
    let made = |pack: Option<&Pack>, width: u8| {
        let (mut s, _) = talk_state(BODY, pack);
        s.converse.script_key = Some((b'a', 3));
        s.ui.text.windows[WindowId::Status.index()].width = width;
        let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
            io.draw(|fb| {
                for y in ROW * CELL..(ROW + 1) * CELL {
                    for x in 0..320 {
                        fb.set(x, y, PAPER);
                    }
                }
            });
            talk::begin(&mut st.borrow_mut(), &io);
            io.present();
        });
        finish(&mut e);
        let s = e.state();
        let (x0, x1) = (22 * CELL, (22 + usize::from(width)) * CELL);
        let fb = e.frame();
        let outside = (ROW * CELL..(ROW + 1) * CELL)
            .flat_map(|y| (0..320).map(move |x| (x, y)))
            .filter(|&(x, y)| (x < x0 || x >= x1) && fb.get(x, y) != Some(PAPER))
            .count();
        let cells: Vec<usize> = s.ui.talk.buttons.iter().map(|b| b.cells.len()).collect();
        (cells, outside, button_pixels(&e))
    };
    for (pack, width, cells) in [
        (None, 17, vec![5, 6]),
        (None, 12, vec![5, 6]),
        (None, 11, vec![3, 4]),
        (None, 8, vec![3, 4]),
        (None, 7, vec![]),
        (Some(&pack), 17, vec![10, 6]),
        (Some(&pack), 13, vec![8, 4]),
        (Some(&pack), 12, vec![]),
    ] {
        let (got, outside, drawn) = made(pack, width);
        assert_eq!(got, cells, "width {width}, pack {}", pack.is_some());
        assert_eq!(outside, 0, "width {width}: nothing drawn outside the row");
        assert_eq!(
            drawn > 0,
            !cells.is_empty(),
            "width {width}: drawn when made"
        );
    }
    println!("buttons fit the row or none: pass");
}

/// Spec 22 Contract 5 (opus M1.1): a press on a key cap moved off it before the release types
/// nothing, and the read goes on to take what is typed alone.
#[test]
fn a_press_moved_off_its_target_before_the_release_types_nothing() {
    let (s, f) = talk_state(BODY, None);
    let mut e = waiting(s, f);
    let (x, y) = button_pixel(&e, 1);
    e.tick(&[at(x, y, 0)], 16);
    e.tick(&[at(x, y, POINTER_LEFT)], 16);
    e.tick(&[at(4, 4, POINTER_LEFT)], 16);
    e.tick(&[at(4, 4, 0)], 16);
    idle(&mut e, 8);
    assert_eq!(e.state().ui.talk.buttons.len(), 2, "still the f7 read");
    e.tick(&typed("job\n"), 16);
    finish(&mut e);
    assert_eq!(
        e.state().converse.input,
        b"job",
        "the line held nothing before"
    );
    println!("press moved off: nothing typed: pass");
}

/// Spec 22 Contract 5 (opus M1.2): a press and its release on the same key cap in ticks apart
/// type its word.
#[test]
fn a_press_and_its_release_ticks_apart_type_the_word() {
    let (s, f) = talk_state(BODY, None);
    let mut e = waiting(s, f);
    let (x, y) = button_pixel(&e, 1);
    e.tick(&[at(x, y, 0)], 16);
    e.tick(&[at(x, y, POINTER_LEFT)], 16);
    idle(&mut e, 3);
    e.tick(&[at(x, y, 0)], 16);
    finish(&mut e);
    assert_eq!(e.state().converse.input, b"name");
    assert!(screen(&e.state()).ends_with("Me name."), "its answer");
    println!("press and release ticks apart: typed: pass");
}

/// Spec 22 Contract 5 (opus M1.3, gpt-c SHOULD 2): after an ASCII letter and a Hangul syllable
/// typed with a pack, a click on Name (a Hangul alias) and a click on a keyword each leave the
/// state and frame of two Backspaces, the word and Enter typed: the line is the word alone.
#[test]
fn after_ascii_and_hangul_typed_a_click_types_its_word_alone() {
    let body: &[u8] = b"Hi @cat. Go.\xf7\xefname\xf6Me.\xb6\xefcat\xf6Purr.\xb6\xeeNo.\xb6";
    let pack = aliased(body, &[("name", &["이름"])]);
    let (s, f) = talk_state(body, Some(&pack));
    let probe = waiting(s.clone(), f.clone());
    let k = probe.state().ui.text.keys.spans.first().cloned();
    let k = k.expect("the keyword shown");
    let key = message_pixel(&probe.state(), k.col + 1, k.row);
    let name = button_pixel(&probe, 1);
    for (xy, word, answer) in [(name, "이름", "Me."), (key, "cat", "Purr.")] {
        let run = |clicks: bool| {
            let mut e = waiting(s.clone(), f.clone());
            e.tick(&[at(xy.0, xy.1, 0)], 16);
            e.tick(&typed("x가"), 16);
            if clicks {
                click(&mut e, xy, POINTER_LEFT);
            } else {
                let mut keys = vec![InputEvent::Key(Key::Backspace); 2];
                keys.extend(typed(&format!("{word}\n")));
                e.tick(&keys, 16);
            }
            finish(&mut e);
            e
        };
        let (clicked, keyed) = (run(true), run(false));
        assert_eq!(clicked.state().converse.input, word.as_bytes());
        assert!(screen(&clicked.state()).ends_with(answer), "its answer");
        assert!(
            *clicked.state() == *keyed.state()
                && clicked.frame().pixels() == keyed.frame().pixels(),
            "{word}: the state and frame of Backspace twice, the word and Enter"
        );
    }
    println!("click after ASCII and Hangul typed: the word alone: pass");
}

/// Spec 22 Contract 5 (opus M1.4): a right click on a key cap types nothing.
#[test]
fn a_right_click_on_a_cap_types_nothing() {
    let (s, f) = talk_state(BODY, None);
    let mut e = waiting(s, f);
    let xy = button_pixel(&e, 1);
    e.tick(&[at(xy.0, xy.1, 0)], 16);
    click(&mut e, xy, 2);
    idle(&mut e, 8);
    e.tick(&typed("job\n"), 16);
    finish(&mut e);
    assert_eq!(e.state().converse.input, b"job");
    println!("right click: nothing typed: pass");
}

/// Spec 22 Contract 4 (opus S2): the player's words typed past the end of the message window's
/// last row scroll it: the keyword moves up a row with its cells, and a click on it there types
/// it, the letters typed taken back first, as the keys do.
#[test]
fn typing_past_the_last_row_moves_a_keyword_up_and_a_click_still_types_it() {
    let body: &[u8] = b"Hi @cat.\n1\n2\n3\n4\n5\n6\n7\xf7\xefcat\xf6Purr.\xb6\xeeNo.\xb6";
    let (s, f) = talk_state(body, None);
    let mut e = waiting(s.clone(), f.clone());
    let (row, col, last) = {
        let st = e.state();
        let w = st.ui.text.window(WindowId::Messages);
        let k = &st.ui.text.keys.spans[0];
        (k.row, k.col, (w.row, w.col, w.height))
    };
    assert_eq!(
        (last.0, last.1),
        (last.2 - 1, 4),
        "after the prompt on the last row"
    );
    let letters = "abcdefghijklm";
    e.tick(&typed(letters), 16);
    let up = e.state().ui.text.keys.spans[0].row;
    assert_eq!(up, row - 1, "a row up with its cells");
    let st = e.state().clone();
    let (x, y) = message_pixel(&st, col + 1, row - 1);
    assert!(talk::target_at(&st, x, y).is_some(), "hit on its new row");
    let (x, y) = message_pixel(&st, col + 1, row);
    assert_eq!(talk::target_at(&st, x, y), None, "none on its old one");
    let xy = message_pixel(&st, col + 1, row - 1);
    let mut keyed = waiting(s, f);
    keyed.tick(&typed(letters), 16);
    for e in [&mut e, &mut keyed] {
        e.tick(&[at(xy.0, xy.1, 0)], 16);
    }
    click(&mut e, xy, POINTER_LEFT);
    let mut keys = vec![InputEvent::Key(Key::Backspace); letters.len()];
    keys.extend(typed("cat\n"));
    keyed.tick(&keys, 16);
    finish(&mut e);
    finish(&mut keyed);
    assert_eq!(e.state().converse.input, b"cat");
    assert!(
        *e.state() == *keyed.state() && e.frame().pixels() == keyed.frame().pixels(),
        "the state and frame of the keys"
    );
    println!("typed past the last row: keyword a row up, still typed: pass");
}

/// Spec 22 Contract 4 (opus S2): the more prompt's blank row forgets the keyword on that row.
#[test]
fn the_more_prompt_s_blank_row_forgets_the_keyword_on_it() {
    let (mut s, _) = talk_state(BODY, None);
    let m = WindowId::Messages.index();
    s.ui.text.windows[m].flags |= crate::ui::text::PAGED;
    s.ui.text.more_rows = 100;
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        for _ in 0..9 {
            crate::ui::printf::printf(&st, &io, b"x\n", &[]).await;
        }
        crate::ui::printf::printf(&st, &io, b"@cat", &[]).await;
        crate::ui::print::put_raw(&st, &io, -1).await;
        st.borrow_mut().ui.text.more_rows = 1;
        crate::ui::printf::printf(&st, &io, b"\n", &[]).await;
    });
    for _ in 0..4 {
        e.tick(&[], 16);
    }
    assert!(!e.finished(), "the more prompt waits");
    let rows: Vec<u8> = e.state().ui.text.keys.spans.iter().map(|k| k.row).collect();
    assert_eq!(rows, [9], "the keyword on the last row");
    e.tick(&[InputEvent::Key(Key::Space)], 16);
    finish(&mut e);
    assert!(
        e.state().ui.text.keys.spans.is_empty(),
        "blanked with its row, nothing scrolled up"
    );
    println!("the more prompt's blank row forgets the keyword: pass");
}

/// Spec 22 Contract 4 (opus S2): the echo drawn over a keyword's cell forgets it, typed or
/// taken back.
#[test]
fn the_echo_over_a_keyword_forgets_it() {
    let (s, _) = talk_state(BODY, None);
    for backspace in [false, true] {
        let mut e = Engine::with_game(s.clone(), Files::default(), move |st, io| async move {
            crate::ui::printf::printf(&st, &io, b"@cat", &[]).await;
            crate::ui::print::put_raw(&st, &io, -1).await;
            let mut s = st.borrow_mut();
            assert_eq!(s.ui.text.keys.spans.len(), 1, "the keyword shown");
            s.ui.text.win_mut().col = 2;
            let c = if backspace { 8 } else { 0x78 };
            crate::ui::keyread::echo(&mut s, &io, c);
        });
        finish(&mut e);
        assert!(
            e.state().ui.text.keys.spans.is_empty(),
            "backspace {backspace}: forgotten"
        );
    }
    println!("the echo over a keyword forgets it: pass");
}

/// Spec 22 Contract 6 (opus NIT 3): a keyword hovered before the read's first key wait puts the
/// prompt's full row out (the line editor's entry put scrolls the window) is drawn back where the
/// scroll took it once the pointer leaves: no cell stays inverse.
#[test]
fn a_keyword_hovered_before_the_entry_scroll_is_drawn_back_after_it() {
    let body: &[u8] = b"Hi @cat.\n1\n2\n3\n4\n5\n6\xf7\xefcat\xf6Purr.\xb6\xeeNo.\xb6";
    let (mut s, f) = talk_state(body, None);
    // Four cells wide, so the prompt `say:` fills its row and the read starts with a put.
    s.ui.text.windows[WindowId::Messages.index()].width = 4;
    let off = at(4, 4, 0);
    // The tick the read begins in: the first that ends with the line editor waiting.
    let mut plain = Engine::with_game(s.clone(), f.clone(), |st, io| async move {
        begin(&super::Cx::new(&st, &io), 0, 3).await;
    });
    let mut first = 0;
    while !plain.state().ui.input.line_input {
        assert!(first < 12, "the read begins");
        plain.tick(&[off], 16);
        first += 1;
    }
    let k = plain.state().ui.text.keys.spans[0].clone();
    let w = *plain.state().ui.text.window(WindowId::Messages);
    assert_eq!(
        (w.row, w.col),
        (w.height - 1, 0),
        "the entry put took a new row"
    );
    // The pointer where the keyword was before that scroll (a row lower), queued at that tick.
    let (x, y) = message_pixel(&plain.state(), k.col + 1, k.row + 1);
    let mut hovered = Engine::with_game(s, f, |st, io| async move {
        begin(&super::Cx::new(&st, &io), 0, 3).await;
    });
    for i in 1..=first {
        hovered.tick(&[if i == first { at(x, y, 0) } else { off }], 16);
    }
    assert_eq!(
        hovered.state().ui.talk.hot,
        Some(Target::Key {
            row: k.row,
            col: k.col
        }),
        "hovered, then moved up with its keyword"
    );
    // The still pointer is off it now, and then the pointer leaves.
    for e in [&mut plain, &mut hovered] {
        idle(e, 2);
        e.tick(&[off], 16);
        idle(e, 2);
    }
    assert!(
        hovered.frame().pixels() == plain.frame().pixels(),
        "no cell left inverse"
    );
    println!("hover across the entry scroll: drawn back: pass");
}

/// Spec 22 Contract 5 (opus NIT 2): a keyword whose cells type nothing types nothing (no empty
/// line); the read goes on.
#[test]
fn a_keyword_that_types_nothing_is_no_click() {
    // `^e`: the control glyph 0x05, which no key types.
    let body: &[u8] = b"See @{^e}.\xf7\xefcat\xf6Purr.\xb6\xeeNo.\xb6";
    let (mut s, f) = talk_state(body, None);
    s.ui.text.set_wide(true);
    let mut e = waiting(s, f);
    let k = e.state().ui.text.keys.spans.first().cloned();
    let k = k.expect("the cell shown in the alert colour");
    assert!(k.word().is_empty(), "a cell no key types");
    let xy = message_pixel(&e.state(), k.col, k.row);
    e.tick(&[at(xy.0, xy.1, 0)], 16);
    click(&mut e, xy, POINTER_LEFT);
    idle(&mut e, 8);
    e.tick(&typed("cat\n"), 16);
    finish(&mut e);
    assert_eq!(
        e.state().converse.input,
        b"cat",
        "the line held nothing before"
    );
    println!("a keyword that types nothing: no click: pass");
}

/// Spec 22 Contract 4 (opus NIT 8): a braced keyword the word wrap breaks at its space is one
/// target, both parts drawn inverse under the pointer, and a click on either part types all of
/// it; two keywords on the two sides of a wrap stay two.
#[test]
fn a_keyword_the_wrap_broke_types_all_of_it_from_either_part() {
    let body: &[u8] = b"Xxxxxxxxxxx @{big cat}.\xf7\xefbig cat\xf6Roar.\xb6\xeeNo.\xb6";
    let (mut s, f) = talk_state(body, None);
    s.ui.text.set_wide(true);
    let e = waiting(s.clone(), f.clone());
    let st = e.state().clone();
    let parts: Vec<(u8, u8, Vec<u8>)> = st
        .ui
        .text
        .keys
        .spans
        .iter()
        .map(|k| (k.row, k.col, k.word()))
        .collect();
    assert_eq!(parts.len(), 2, "two parts");
    assert_eq!(
        (parts[0].2.as_slice(), parts[1].2.as_slice()),
        (&b"big"[..], &b"cat"[..])
    );
    assert_eq!(
        (parts[1].0, parts[1].1),
        (parts[0].0 + 1, 0),
        "the rest at the next row's start"
    );
    let pixels = [
        message_pixel(&st, parts[0].1 + 1, parts[0].0),
        message_pixel(&st, 1, parts[1].0),
    ];
    let targets: Vec<_> = pixels
        .iter()
        .map(|&(x, y)| talk::target_at(&st, x, y))
        .collect();
    assert!(
        targets[0].is_some() && targets[0] == targets[1],
        "one target"
    );
    // Hovered by its rest: both parts inverse (the leftmost pixel of each cell the alert colour).
    let mut hovered = waiting(s.clone(), f.clone());
    hovered.tick(&[at(pixels[1].0, pixels[1].1, 0)], 16);
    let alert = st.ui.tables.colours.alert;
    let w = *st.ui.text.window(WindowId::Messages);
    let fb = hovered.frame();
    for k in &hovered.state().ui.text.keys.spans {
        for i in 0..k.cells.len() {
            let x = (usize::from(w.x + k.col) + i) * CELL;
            let y = usize::from(w.y + k.row) * CELL;
            assert_eq!(fb.get(x, y), Some(alert), "inverse: row {} cell {i}", k.row);
        }
    }
    for xy in pixels {
        let (e, same) = press_against_typing(&s, &f, |_| xy, "big cat\n");
        assert!(same, "the state and frame of typing it");
        assert_eq!(e.state().converse.input, b"big cat");
    }
    // `@a` ending a row and `@b` starting the next, a space of the text's ink between: two.
    let body: &[u8] = b"Xxxxxxxxxxxxxx @ab @cd.\xf7\xeeNo.\xb6";
    let (s, f) = talk_state(body, None);
    let e = waiting(s, f);
    let st = e.state();
    let words: Vec<Vec<u8>> = st.ui.text.keys.spans.iter().map(|k| k.word()).collect();
    assert_eq!(words, [b"ab".to_vec(), b"cd".to_vec()]);
    let (a, b) = (&st.ui.text.keys.spans[0], &st.ui.text.keys.spans[1]);
    assert_eq!((b.row, b.col), (a.row + 1, 0), "on the two sides of a wrap");
    assert_eq!(st.ui.text.keys.word(b.row, 0), b"cd", "two words");
    println!("wrapped keyword: one target, typed whole from either part: pass");
}

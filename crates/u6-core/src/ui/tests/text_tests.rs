//! printf directives, escapes, word wrap and scrolling of the message scroll.

use super::*;

fn top(fmt: &'static [u8], args: &'static [Arg<'static>]) -> String {
    scroll_row(&print(state(), fmt, args), 14)
        .trim_end()
        .to_string()
}

#[test]
fn printf_numbers_padding_and_plural() {
    assert_eq!(top(b"%d apple\\s.", &[Arg::Word(1)]), "1 apple.");
    assert_eq!(top(b"%d apple\\s.", &[Arg::Word(3)]), "3 apples.");
    // After `/` letters print only for a singular; any other character ends the escape.
    assert_eq!(top(b"%d a/bc d", &[Arg::Word(1)]), "1 abc d");
    assert_eq!(top(b"%d a/bc d", &[Arg::Word(2)]), "2 a d");
    assert_eq!(top(b"[%2d]", &[Arg::Word(5)]), "[ 5]");
    assert_eq!(top(b"[%-3d]", &[Arg::Word(5)]), "[5  ]");
    assert_eq!(top(b"[%03d]", &[Arg::Word(5)]), "[005]");
    assert_eq!(
        top(
            b"[%x%/%o%/%u]",
            &[Arg::Word(255), Arg::Word(8), Arg::Word(9)]
        ),
        "[FF/10/9]"
    );
    // A bare `/` is the singular escape: it prints nothing itself.
    assert_eq!(top(b"[%x/%o]", &[Arg::Word(255), Arg::Word(8)]), "[FF10]");
    // %c keeps the field length 0: the padding is the whole width.
    assert_eq!(top(b"[%3c]", &[Arg::Word(0x78)]), "[   x]");
    assert_eq!(top(b"%s!", &[Arg::Str(b"ab")]), "ab!");
    // Upper-case directives are the same directives.
    assert_eq!(top(b"%D", &[Arg::Word(7)]), "7");
}

#[test]
fn printf_unknown_directive_prints_the_character_unescaped() {
    // `/`, `>` and `%` alone would be escapes; as directives they print literally.
    assert_eq!(top(b"7%/9", &[]), "7/9");
    assert_eq!(top(b"%s:%>", &[Arg::Str(b"Iolo")]), "Iolo:>");
    assert_eq!(top(b"100%%", &[]), "100%");
}

#[test]
fn escapes_variables_hidden_text_and_glyphs() {
    let mut s = state();
    s.converse.ints[3] = 2;
    s.converse.ints[10] = 1;
    s.converse.strs[1] = crate::converse::StrRef::Buffer(0);
    s.converse.buffers[0] = b"xyz".to_vec();
    let e = print(s, b"#3 thing\\s #a thing\\s $1!", &[]);
    assert_eq!(scroll_row(&e, 14).trim_end(), "2 things 1 thing");
    assert_eq!(scroll_row(&e, 15).trim_end(), "xyz!");
    assert_eq!(top(b"a&bc&d", &[]), "ad");
    // `^c` is the control glyph c & 0x1f; `<` / `>` select the runic glyphs (code | 0x80).
    let e = print(state(), b"^a<ab>c", &[]);
    let fb = e.frame();
    assert_eq!([cell(fb, 22, 14, INK), cell(fb, 23, 14, INK)], [0x01, 0xe1]);
    assert_eq!([cell(fb, 24, 14, INK), cell(fb, 25, 14, INK)], [0xe2, b'c']);
}

#[test]
fn back_quote_capitalises_and_at_marks_a_keyword() {
    assert_eq!(top(b"`hello `world", &[]), "Hello World");
    let e = print(state(), b"say @name. ok", &[]);
    let fb = e.frame();
    assert_eq!(row_text(fb, 26..30, 14, ALERT), "name");
    // The keyword ends at a character of the end set: back to the text colour.
    assert_eq!(row_text(fb, 30..34, 14, INK), ". ok");
    assert_eq!(cell(fb, 26, 14, INK), 0);
}

#[test]
fn word_wrap_at_spaces_hyphens_and_hard_breaks() {
    let e = print(state(), b"aaaa bbbb cccc dddd eeee", &[]);
    assert_eq!(scroll_row(&e, 14).trim_end(), "aaaa bbbb cccc");
    assert_eq!(scroll_row(&e, 15).trim_end(), "dddd eeee");
    let e = print(state(), b"aaaaaaaa-bbbbbbbbbb", &[]);
    assert_eq!(scroll_row(&e, 14).trim_end(), "aaaaaaaa-");
    assert_eq!(scroll_row(&e, 15).trim_end(), "bbbbbbbbbb");
    let e = print(state(), b"aaaaaaaaaaaaaaaaaaaa", &[]);
    assert_eq!(scroll_row(&e, 14), "a".repeat(17));
    assert_eq!(scroll_row(&e, 15).trim_end(), "aaa");
    // The finished lines are in the transcript; the flushed tail is not (no newline yet).
    let s = e.state();
    assert_eq!(s.ui.messages, ["a".repeat(17)]);
    assert_eq!((s.ui.text.win().row, s.ui.text.win().col), (1, 3));
}

#[test]
fn full_window_scrolls_up_one_row() {
    let e = run(state(), &[], 4, |st, io| async move {
        for i in 0..11u16 {
            printf(&st, &io, b"l%d\n", &[Arg::Word(i)]).await;
        }
        io.present();
    });
    // Ten rows: each newline on the last row scrolls, so l0 and l1 are gone.
    assert_eq!(scroll_row(&e, 14).trim_end(), "l2");
    assert_eq!(scroll_row(&e, 22).trim_end(), "l10");
    assert_eq!(scroll_row(&e, 23).trim_end(), "");
    // The scroll cleared the last row to the paper colour.
    assert_eq!(e.frame().get(22 * 8, 23 * 8 + 3), Some(PAPER));
    let s = e.state();
    assert_eq!(s.ui.messages.len(), 11);
    assert_eq!(s.ui.messages[10], "l10");
}

#[test]
fn star_waits_for_a_key() {
    let game = |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
        printf(&st, &io, b"a*b", &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    };
    let e = run(state(), &[], 6, game);
    assert!(!e.finished(), "waits at `*`");
    let e = run(state(), &[Key::Char('a')], 6, game);
    assert!(e.finished());
    assert_eq!(scroll_row(&e, 14).trim_end(), "ab");
}

#[test]
fn main_windows_match_the_corner_cells_main_passes() {
    let s = state();
    let w = |id| *s.ui.text.window(id);
    let geo = |w: TextWindow| (w.x, w.y, w.width, w.height);
    assert_eq!(geo(w(WindowId::Screen)), (0, 0, 40, 25));
    assert_eq!(geo(w(WindowId::Status)), (22, 1, 17, 12));
    assert_eq!(geo(w(WindowId::Messages)), (22, 14, 17, 10));
    assert_eq!(
        w(WindowId::Messages).flags,
        text::WRAP | text::SCROLL | text::CAPITALISE
    );
    assert_eq!(
        (w(WindowId::Status).fg, w(WindowId::Status).bg),
        (INK, PAPER)
    );
    assert_eq!(s.ui.text.current, WindowId::Messages);
}

#[test]
fn paged_window_waits_for_a_key_when_full() {
    let mut s = state();
    s.ui.text.win_mut().flags |= text::PAGED;
    s.ui.text.more_rows = 10;
    let game = |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
        for i in 0..12u16 {
            printf(&st, &io, b"p%d\n", &[Arg::Word(i)]).await;
        }
        io.present();
    };
    let e = run(s.clone(), &[], 6, game);
    assert!(!e.finished(), "more prompt after nine lines");
    assert_eq!(e.state().ui.messages.len(), 9);
    assert_eq!(e.state().ui.input.cursor_glyph, 1);
    // Space ends the prompt; the count is rearmed, the remaining lines follow.
    let e = run(s.clone(), &[Key::Space], 6, game);
    assert!(e.finished());
    assert_eq!(e.state().ui.messages.len(), 12);
    assert_eq!(e.state().ui.input.cursor_glyph, 5);
    // A key outside space / return / escape does not end it.
    let e = run(s, &[Key::Char('a')], 6, game);
    assert!(!e.finished());
}

#[test]
fn set_cursor_clamps_to_the_window_and_starts_the_line_there() {
    let e = run(state(), &[], 4, |st, io| async move {
        panels::set_cursor(&st, &io, 40, -3).await;
        let (col, row) = {
            let s = st.borrow();
            (s.ui.text.win().col, s.ui.text.win().row)
        };
        assert_eq!((col, row), (17, 0));
        panels::set_cursor(&st, &io, 5, 2).await;
        printf(&st, &io, b"xy", &[]).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    });
    assert!(e.finished());
    assert_eq!(&scroll_row(&e, 16)[5..7], "xy");
}

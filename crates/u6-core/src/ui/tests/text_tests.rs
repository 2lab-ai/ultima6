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
    assert_eq!(top(b"%s:%>", &[Arg::Str(b"Wxyz")]), "Wxyz:>");
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

/// A paper of the screen's size all [`PAPER`] but `other` pixels of colour `c`, its colours
/// through the identity table (spec 07 HD0, [`text::Paper`]), and the bitmap it is drawn from.
fn paper(other: &[(usize, usize)], c: u8) -> text::Paper {
    let mut pixels = vec![PAPER; 320 * 200];
    for &(x, y) in other {
        pixels[y * 320 + x] = c;
    }
    let bmp = u6_formats::bitmap::Bitmap {
        width: 320,
        height: 200,
        pixels,
    };
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    text::Paper::drawn(&identity, &bmp)
}

/// A frame with the plane and colours distinct per index.
fn coloured_plane() -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.enable_src();
    for (i, c) in fb.palette.iter_mut().enumerate() {
        *c = u6_render::Rgb::new(i as u8, 255 - i as u8, 9);
    }
    fb
}

/// The HD font mode's list of `fb` and its symbols' rows.
fn listed(fb: &Framebuffer) -> (Vec<u6_render::hd::HdGlyph>, Vec<[u8; 8]>) {
    let mut c = u6_render::hd::Compositor::new();
    c.set_font_mode(true);
    c.compose(fb, &mut u6_render::hd::NoImages, u6_render::hd::Scale::X2);
    (c.glyphs().to_vec(), c.symbol_rows().to_vec())
}

/// `fb` as the key wait's symbol named its pixels before spec 11 rev 3: each pixel named a
/// symbol's ink ([`u6_render::FLAG_SYMBOL`]) named the paper by its rule instead
/// ([`text::Paper::over`]), the colours as they are.
fn as_before_rev3(fb: &Framebuffer, paper: &text::Paper) -> Framebuffer {
    let mut old = fb.clone();
    for (i, s) in fb.src().unwrap().iter().enumerate() {
        if s.kind == u6_render::Kind::Glyph && s.flags & u6_render::FLAG_SYMBOL != 0 {
            let (x, y) = (i % u6_render::WIDTH, i / u6_render::WIDTH);
            let colour = fb.pixels()[i];
            old.set_src(x, y, colour, paper.over(x, y, colour));
        }
    }
    old
}

/// Spec 11 Design 2 (rev 3) on the text path, synthetic: over the paper, two made-up letters and
/// the key wait's symbol in the next cell ([`text::draw_symbol`]) are listed as three cells, the
/// third the symbol (its glyph, ink and paper) with its rows; before rev 3 two (the sources the
/// symbol named then list the letters alone). An ink pixel of the symbol of the paper's own colour
/// there names the paper as before and is still one of its rows. Without the paper's colours the
/// symbol is classic and the letters alone are listed. One pixel of the cell written again
/// classic: not listed. With the HD font off, the HD frame is byte for byte the one of the
/// sources of before rev 3.
#[test]
fn a_symbol_after_two_letters_over_the_paper_is_listed_third() {
    use u6_render::hd::{HdGlyph, NoImages, SYMBOL, Scale};
    let font = font();
    let w = *state().ui.text.win();
    let fonts = (Some(&font), None);
    // The symbol's cell (24, 14): ink at columns 5 and 7 (glyph 5's rows); the paper of the ink's
    // colour under one of them.
    let edge = (24 * 8 + 5, 14 * 8 + 3);
    for (what, kept) in [
        ("the paper", Some(paper(&[], INK))),
        ("an ink pixel the paper's colour", Some(paper(&[edge], INK))),
        ("no paper", None),
    ] {
        let mut fb = coloured_plane();
        if let Some(p) = &kept {
            text::fill_rect(&mut fb, 0, 0, 319, 199, PAPER, Some(p));
        }
        text::draw_char(&mut fb, Some(&font), None, &w, u16::from(b'a'), 22, 14);
        text::draw_char(&mut fb, Some(&font), None, &w, u16::from(b'b'), 23, 14);
        text::draw_symbol(&mut fb, fonts, &w, 5, (24, 14), kept.as_ref());
        let letter = |col: u16, code: u8| HdGlyph {
            x: col * 8,
            y: 112,
            code: u16::from(code),
            ink: fb.palette[usize::from(INK)],
            paper: fb.palette[usize::from(PAPER)],
        };
        let letters = vec![letter(22, b'a'), letter(23, b'b')];
        let Some(p) = &kept else {
            assert_eq!(listed(&fb), (letters, vec![]), "{what}: the letters alone");
            continue;
        };
        let symbol = HdGlyph {
            code: SYMBOL + 5,
            ..letter(24, 0)
        };
        let mut three = letters.clone();
        three.push(symbol);
        assert_eq!(
            listed(&fb),
            (three, vec![[5; 8]]),
            "{what}: three cells, the third the symbol"
        );
        let old = as_before_rev3(&fb, p);
        assert_eq!(listed(&old), (letters, vec![]), "{what}: before rev 3, two");
        for scale in [Scale::X2, Scale::X4] {
            let (mut a, mut b) = (
                u6_render::hd::Compositor::new(),
                u6_render::hd::Compositor::new(),
            );
            a.compose(&fb, &mut NoImages, scale);
            b.compose(&old, &mut NoImages, scale);
            assert!(
                a.rgba() == b.rgba(),
                "{what}, {scale:?}: the HD font off, as before rev 3"
            );
        }
        let mut broken = fb.clone();
        broken.set(24 * 8 + 1, 14 * 8 + 6, PAPER);
        assert_eq!(
            listed(&broken).0.len(),
            2,
            "{what}: a pixel written again classic"
        );
    }
}

/// Spec 11 Design 2 (rev 3) through the key waits over the paper: after a line ("ab", no newline)
/// the line editor's cursor (glyphs 5..=8) is listed as a symbol in the next cell, its rows its
/// glyph's; at the more prompt of a paged window its mark (glyphs 1..=4) is listed as a symbol.
/// Both draw through [`text::draw_symbol`].
#[test]
fn the_key_waits_list_their_symbols_over_the_paper() {
    use u6_render::hd::symbol_glyph;
    let kept = paper(&[], INK);
    let symbols = |fb: &Framebuffer| -> Vec<(u16, u16, u8, [u8; 8])> {
        let (list, rows) = listed(fb);
        let at: Vec<_> = list
            .iter()
            .filter_map(|g| symbol_glyph(g.code).map(|s| (g.x, g.y, s)))
            .collect();
        assert_eq!(at.len(), rows.len(), "one row set per symbol");
        at.into_iter()
            .zip(rows)
            .map(|((x, y, s), r)| (x, y, s, r))
            .collect()
    };
    let mut s = state();
    s.ui.paper = Some(kept.clone());
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        let p = st.borrow().ui.paper.clone();
        io.draw(|fb| text::fill_rect(fb, 0, 0, 319, 199, PAPER, p.as_ref()));
        printf(&st, &io, b"ab", &[]).await;
        let _ = keyread::read_line(&st, &io, 8).await;
    });
    e.enable_src_plane();
    for _ in 0..6 {
        e.tick(&[], 16);
    }
    let seen = symbols(e.frame());
    assert_eq!(seen.len(), 1, "the line editor's cursor listed");
    let (x, y, glyph, rows) = seen[0];
    assert_eq!((x, y), (24 * 8, 14 * 8), "after the line");
    assert!(
        (5..=8).contains(&glyph) && rows == [glyph; 8],
        "the cursor's glyph"
    );
    // The more prompt of a paged window.
    let mut s = state();
    s.ui.paper = Some(kept);
    s.ui.text.win_mut().flags |= text::PAGED;
    s.ui.text.more_rows = 10;
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        let p = st.borrow().ui.paper.clone();
        io.draw(|fb| text::fill_rect(fb, 0, 0, 319, 199, PAPER, p.as_ref()));
        for i in 0..12u16 {
            printf(&st, &io, b"p%d\n", &[Arg::Word(i)]).await;
        }
        io.present();
    });
    e.enable_src_plane();
    for _ in 0..6 {
        e.tick(&[], 16);
    }
    assert_eq!(
        e.state().ui.input.cursor_glyph,
        1,
        "precondition: at the more prompt"
    );
    let seen = symbols(e.frame());
    assert_eq!(seen.len(), 1, "the more prompt's mark listed");
    let (_, _, glyph, rows) = seen[0];
    assert!(
        (1..=4).contains(&glyph) && rows == [glyph; 8],
        "the mark's glyph"
    );
}

/// The paper bitmap of `paper(other, c)` as the compositor's image of the paper (spec 07 HD0:
/// [`u6_render::BITMAP_PAPER`]), drawn through the identity table.
struct PaperImage(Vec<u8>);

impl u6_render::hd::Images for PaperImage {
    fn image(&mut self, kind: u6_render::Kind, id: u16) -> Option<u6_render::hd::Image<'_>> {
        (kind == u6_render::Kind::Bitmap && id == u6_render::BITMAP_PAPER).then(|| {
            u6_render::hd::Image {
                ver: 0,
                width: 320,
                height: 200,
                pixels: &self.0,
                transparent: None,
            }
        })
    }
}

/// Spec 11 Design 2 (rev 3), the old cell of a symbol over a paper of more than one colour (the
/// key wait's symbol after two letters, the HD font mode on, the paper's HD image at 2×): one ink
/// pixel of the symbol where the paper's own colour is the ink's (named the paper, unflagged) and
/// one where it is another (named a symbol) are both in the symbol's rows; and every pixel of the
/// cell is composed as the same frame with no symbol drawn there, the paper alone.
///
///
/// The ink pixel of the paper's colour names the paper unflagged (as before rev 3, so the HD
/// font off is unchanged); in the listed cell the paper's HD block shows only where the paper's
/// colour is the cell's paper colour, else the cell's flat paper (K12 review round 2: before it
/// the paper's own block, the ink's colour, showed there).
#[test]
fn a_symbols_old_cell_over_a_paper_of_two_colours_is_the_paper_alone() {
    use u6_render::hd::{Compositor, Scale, symbol_glyph};
    let font = font();
    let w = *state().ui.text.win();
    let (x0, y0) = (24 * 8, 14 * 8);
    // Glyph 5's ink is at columns 5 and 7: on row 3 the paper is the ink's colour under column 5
    // and another under column 7, and under a clear pixel another again.
    let same = (x0 + 5, y0 + 3);
    let other = (x0 + 7, y0 + 3);
    let clear = (x0 + 2, y0 + 1);
    let mut pixels = vec![PAPER; 320 * 200];
    pixels[same.1 * 320 + same.0] = INK;
    pixels[other.1 * 320 + other.0] = ALERT;
    pixels[clear.1 * 320 + clear.0] = ALERT;
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    let bmp = u6_formats::bitmap::Bitmap {
        width: 320,
        height: 200,
        pixels: pixels.clone(),
    };
    let kept = text::Paper::drawn(&identity, &bmp);
    let frame = |symbol: bool| {
        let mut fb = coloured_plane();
        crate::gfx::blit::blit_bmp(&identity, &mut fb, &bmp, 0, 0, u6_render::BITMAP_PAPER);
        text::fill_rect(&mut fb, 0, 0, 319, 199, PAPER, Some(&kept));
        text::draw_char(&mut fb, Some(&font), None, &w, u16::from(b'a'), 22, 14);
        text::draw_char(&mut fb, Some(&font), None, &w, u16::from(b'b'), 23, 14);
        if symbol {
            text::draw_symbol(&mut fb, (Some(&font), None), &w, 5, (24, 14), Some(&kept));
        }
        fb
    };
    let (with, without) = (frame(true), frame(false));
    let src = |(x, y): (usize, usize)| with.src_at(x, y).unwrap();
    assert_eq!(
        src(same),
        text::Paper::src(same.0, same.1),
        "precondition: the ink pixel of the paper's colour names the paper"
    );
    assert_eq!(
        src(other).kind,
        u6_render::Kind::Glyph,
        "precondition: the other ink pixel names the symbol"
    );
    let compose = |fb: &Framebuffer| {
        let mut c = Compositor::new();
        c.set_font_mode(true);
        c.compose(fb, &mut PaperImage(pixels.clone()), Scale::X2);
        c
    };
    let (a, b) = (compose(&with), compose(&without));
    let symbols: Vec<_> = a
        .glyphs()
        .iter()
        .filter(|g| symbol_glyph(g.code).is_some())
        .collect();
    assert_eq!(symbols.len(), 1, "the symbol listed");
    let rows = a.symbol_rows()[0];
    for (what, (x, y)) in [
        ("the ink pixel of the paper's colour", same),
        ("the other ink pixel", other),
    ] {
        assert!(
            rows[y - y0] & (0x80 >> (x - x0)) != 0,
            "(a) {what} in the symbol's rows"
        );
    }
    let block = |c: &Compositor, (x, y): (usize, usize)| -> Vec<u32> {
        let ow = u6_render::WIDTH * 2;
        (0..4)
            .map(|k| c.rgba()[(y * 2 + k / 2) * ow + x * 2 + k % 2])
            .collect()
    };
    let differ: Vec<(usize, usize)> = (0..64)
        .map(|k| (x0 + k % 8, y0 + k / 8))
        .filter(|&p| block(&a, p) != block(&b, p))
        .collect();
    assert_eq!(
        differ,
        [],
        "(b) every pixel of the cell composed as the paper alone (left: those not; the ink pixel \
         of the paper's colour is {same:?}, the other {other:?})"
    );
}

//! Spec 08 Design 4 (slice K5a): the line editor with text an input method committed
//! ([`crate::engine::InputEvent::Text`]), on the synthetic font and a synthetic Hangul table: the
//! player's words with a pack take Hangul syllables as UTF-8, one cell each, `max` a byte budget;
//! without a pack, and in every other read, a syllable is dropped as if it had not come (the
//! frames those of no event); printable ASCII typed as text is the press of its key; a key wait
//! takes no text and leaves none queued.

use std::rc::Rc;

use u6_formats::font::{HANGUL_GLYPHS, HangulFont};

use super::*;
use crate::locale::Pack;
use crate::ui::wide::Language;

/// Syllable i's rows (as the wide tests draw them: never eight equal rows, so never a glyph of
/// the synthetic `u6.ch`).
fn rows(i: usize) -> [u8; 8] {
    [0xa5, (i >> 8) as u8, i as u8, 0x5a, 0xff, 0, 0x81, 0x3c]
}

/// The 8 rows of screen cell (col, row) in ink `fg` (bit 7 = leftmost).
fn glyph(fb: &Framebuffer, col: usize, row: usize, fg: u8) -> [u8; 8] {
    std::array::from_fn(|dy| {
        (0..CELL).fold(0, |c, dx| {
            let on = fb.get(col * CELL + dx, row * CELL + dy) == Some(fg & 0x0f);
            (c << 1) | u8::from(on)
        })
    })
}

/// The test state; with `pack`, a locale pack installed as `install` leaves the text layer
/// (`wide` on, the Hangul table), the pack holding nothing these reads use.
fn typing(pack: bool) -> State {
    let mut s = state();
    if pack {
        let table = HangulFont::from_glyphs((0..HANGUL_GLYPHS).map(rows).collect()).unwrap();
        s.locale.pack = Some(Rc::new(
            Pack::new(Language::Korean, "test/1", 0, []).unwrap(),
        ));
        s.locale.hangul = Some(Rc::new(table));
        crate::locale::apply(&mut s);
    }
    s
}

fn text(c: char) -> InputEvent {
    InputEvent::Text(c)
}

fn key(k: Key) -> InputEvent {
    InputEvent::Key(k)
}

/// The line editor (`words`: the player's words, [`keyread::read_words`]; else
/// [`keyread::read_line`]) for at most `max` bytes over `s`, given `ticks` (one tick each, then
/// empty ones until it ends): after every tick the frame, the state and the events still queued;
/// the count goes to word variable 0 and the line to the converse buffer 0 (test scratch).
fn edit(
    s: State,
    words: bool,
    max: i16,
    ticks: &[Vec<InputEvent>],
) -> (Engine, Vec<(Vec<u8>, State, usize)>) {
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        let (n, line) = if words {
            keyread::read_words(&st, &io, max).await
        } else {
            keyread::read_line(&st, &io, max).await
        };
        io.present();
        let mut s = st.borrow_mut();
        s.converse.ints[0] = n as u16;
        s.converse.buffers[0] = line;
    });
    let mut seen = Vec::new();
    for t in ticks.iter().map(Vec::as_slice).chain([&[][..]; 4]) {
        e.tick(t, 16);
        seen.push((
            e.frame().pixels().to_vec(),
            e.state().clone(),
            e.io().pending_events(),
        ));
    }
    (e, seen)
}

#[test]
fn the_words_with_a_pack_take_hangul_as_utf8_one_cell_each_and_backspace_takes_a_syllable() {
    let syl = |c: char| rows(c as usize - 0xac00);
    let (bs, enter) = (key(Key::Backspace), key(Key::Enter));
    let typed = vec![
        text('가'),
        text('나'),
        bs,
        text('다'),
        key(Key::Char('a')),
        enter,
    ];
    let (e, _) = edit(typing(true), true, 8, &[typed]);
    assert!(e.finished());
    let st = e.state();
    assert_eq!(
        st.converse.buffers[0],
        "가다a".as_bytes(),
        "UTF-8, 나 taken back whole"
    );
    assert_eq!(st.converse.ints[0], 7, "the count is in bytes");
    let fb = e.frame();
    assert_eq!(glyph(fb, 22, 14, INK), syl('가'), "one cell from the table");
    assert_eq!(
        glyph(fb, 23, 14, INK),
        syl('다'),
        "the next cell, 나 erased"
    );
    assert_eq!(cell(fb, 24, 14, INK), b'a');
    assert_eq!(row_text(fb, 25..39, 14, INK).trim_end(), "");
    assert_eq!(
        st.ui.text.transcript_line,
        [0xac00, 0xb2e4, u16::from(b'a')],
        "the transcript shows the line as the screen does"
    );
    drop(st);
    // `max` stays a byte budget: a syllable that would pass it is not taken, a byte still fits.
    let typed = [
        text('가'),
        text('나'),
        text('다'),
        key(Key::Char('a')),
        key(Key::Char('b')),
        enter,
    ];
    let (e, _) = edit(typing(true), true, 7, &[typed.to_vec()]);
    assert_eq!(e.state().converse.buffers[0], "가나a".as_bytes());
    assert_eq!(e.state().converse.ints[0], 7);
    let fb = e.frame();
    assert_eq!(glyph(fb, 23, 14, INK), syl('나'));
    assert_eq!(cell(fb, 24, 14, INK), b'a');
    assert_eq!(row_text(fb, 25..39, 14, INK).trim_end(), "");
    // Escape erases one cell per character and gives -1.
    let (e, _) = edit(
        typing(true),
        true,
        8,
        &[vec![text('가'), key(Key::Char('a')), key(Key::Escape)]],
    );
    assert_eq!(e.state().converse.ints[0], 0xffff);
    assert!(e.state().converse.buffers[0].is_empty());
    assert_eq!(row_text(e.frame(), 22..39, 14, INK).trim_end(), "");
}

#[test]
fn hangul_without_a_pack_or_in_another_read_is_dropped_and_the_frames_are_those_of_no_event() {
    let a = key(Key::Char('a'));
    let s = key(Key::Char('s'));
    let with = [
        vec![a],
        vec![text('가')],
        vec![text('나'), s, text('다')],
        vec![],
        vec![key(Key::Enter)],
    ];
    let without = [vec![a], vec![], vec![s], vec![], vec![key(Key::Enter)]];
    // The player's words without a pack; another read (a number, a name) with one.
    for (pack, words) in [(false, true), (true, false)] {
        let (e, typed) = edit(typing(pack), words, 8, &with);
        let (_, none) = edit(typing(pack), words, 8, &without);
        for (i, ((fa, sa, qa), (fb, sb, qb))) in typed.iter().zip(&none).enumerate() {
            assert!(
                fa == fb,
                "tick {i}: the frame of no event ({pack}, {words})"
            );
            assert!(
                sa == sb,
                "tick {i}: the state of no event ({pack}, {words})"
            );
            assert_eq!((*qa, *qb), (0, 0), "tick {i}: nothing left queued");
        }
        assert_eq!(e.state().converse.buffers[0], b"as");
    }
}

#[test]
fn printable_ascii_typed_as_text_is_the_press_of_its_key() {
    let keys = [
        vec![
            key(Key::Char('a')),
            key(Key::Char('S')),
            key(Key::Space),
            key(Key::Char('1')),
        ],
        vec![key(Key::Enter)],
    ];
    let texts = [
        vec![text('a'), text('S'), text(' '), text('1')],
        vec![key(Key::Enter)],
    ];
    for (pack, words) in [(false, false), (false, true), (true, true)] {
        let (e, by_key) = edit(typing(pack), words, 8, &keys);
        let (_, by_text) = edit(typing(pack), words, 8, &texts);
        assert!(by_key == by_text, "every tick the same ({pack}, {words})");
        assert_eq!(e.state().converse.buffers[0], b"aS 1");
    }
}

#[test]
fn a_key_wait_takes_no_text_and_leaves_none_queued() {
    let mut e = Engine::with_game(typing(true), Files::default(), |st, io| async move {
        let k = wait::read_key(&st, &io).await;
        st.borrow_mut().converse.ints[0] = k;
    });
    e.tick(&[text('가'), text('x')], 16);
    e.tick(&[], 16);
    assert!(!e.finished(), "no key came");
    assert_eq!(
        e.io().pending_events(),
        0,
        "the text dropped where it was read"
    );
    e.tick(&[text('나'), key(Key::Char('a'))], 16);
    for _ in 0..8 {
        e.tick(&[], 16);
    }
    assert!(e.finished());
    assert_eq!(
        e.state().converse.ints[0],
        u16::from(b'A'),
        "the key behind it"
    );
    assert_eq!(e.io().pending_events(), 0);
}

/// Spec 11 Design 3 (rev 3), through the line editor: its echo names each cell it draws typed
/// ([`u6_render::FLAG_TYPED`] on all its pixels: two spaces, a letter typed after Backspace, a
/// syllable with a pack); the cursor's cell erased when the line ends is the game's space (no
/// flag); the line read is the same. The HD font mode lists the typed spaces as
/// [`u6_render::hd::TYPED_SPACE`], the typed letter and syllable as their codes, the erased cell
/// as a space. (The synthetic font's space here is blank, a whole space.) Backspace's own blank
/// is never in a frame the line editor shows (the cursor is drawn there at once):
/// [`backspace_blanks_a_typed_cell_as_the_games_space`] reads it where it is drawn.
#[test]
fn the_line_editors_echo_names_its_cells_typed() {
    use u6_render::hd::{Compositor, NoImages, Scale, TYPED_SPACE};
    use u6_render::{FLAG_TYPED, Kind};
    let bs = key(Key::Backspace);
    let typed = vec![
        key(Key::Space),
        key(Key::Space),
        key(Key::Char('a')),
        key(Key::Char('s')),
        bs,
        bs,
        key(Key::Char('a')),
        text('가'),
        key(Key::Enter),
    ];
    let mut s = typing(true);
    s.ui.font.as_mut().unwrap().glyphs[0x20] = [0; 8];
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        let (n, line) = keyread::read_words(&st, &io, 16).await;
        io.present();
        let mut s = st.borrow_mut();
        s.converse.ints[0] = n as u16;
        s.converse.buffers[0] = line;
    });
    e.enable_src_plane();
    e.tick(&typed, 16);
    for _ in 0..6 {
        e.tick(&[], 16);
    }
    assert!(e.finished());
    assert_eq!(
        e.state().converse.buffers[0],
        "  a가".as_bytes(),
        "the line read as before"
    );
    let fb = e.frame();
    // Each cell's 64 sources: (kind, code, typed) when all agree.
    let cell = |col: usize| {
        let at = |k: usize| fb.src_at(col * 8 + k % 8, 14 * 8 + k / 8).unwrap();
        let s0 = at(0);
        let same = (0..64).all(|k| {
            let s = at(k);
            s.kind == s0.kind && s.id == s0.id && s.flags & FLAG_TYPED == s0.flags & FLAG_TYPED
        });
        same.then_some((s0.kind, s0.id, s0.flags & FLAG_TYPED != 0))
    };
    let glyph = |code: u16, typed: bool| Some((Kind::Glyph, code, typed));
    assert_eq!(cell(22), glyph(0x20, true), "a typed space");
    assert_eq!(cell(23), glyph(0x20, true), "a typed space");
    assert_eq!(
        cell(24),
        glyph(u16::from(b'a'), true),
        "a letter typed after Backspace"
    );
    assert_eq!(cell(25), glyph(0xac00, true), "a syllable typed");
    assert_eq!(
        cell(26),
        glyph(0x20, false),
        "the cursor's cell erased at the line's end"
    );
    let mut c = Compositor::new();

    c.compose(fb, &mut NoImages, Scale::X2);
    let codes: Vec<(u16, u16)> = c
        .glyphs()
        .iter()
        .filter(|g| g.y == 112)
        .map(|g| (g.x / 8, g.code))
        .collect();
    assert_eq!(
        codes[..5],
        [
            (22, TYPED_SPACE),
            (23, TYPED_SPACE),
            (24, u16::from(b'a')),
            (25, 0xac00),
            (26, 0x20)
        ],
        "listed: the typed spaces apart, the letters as they are"
    );
}

/// Spec 11 Design 3 (rev 3): the blank Backspace draws ([`keyread::echo`] of 8) is the game's own
/// space, every one of its 64 sources a whole space's with no [`u6_render::FLAG_TYPED`] (and
/// [`u6_render::FLAG_PAPER`] over the paper, as any glyph's), read right after the echo, before
/// anything else draws there: over a typed space, over a typed letter, and back across the
/// row's end (the echo wrapped, Backspace blanks the last cell of the row above). Each cell was
/// typed, all 64 sources flagged, just before. With no paper and over the paper.
#[test]
fn backspace_blanks_a_typed_cell_as_the_games_space() {
    use std::cell::RefCell;
    use u6_render::{FLAG_INK, FLAG_PAPER, FLAG_TYPED, Kind, Src};
    type Shot = (&'static str, (u16, u16), Vec<Src>);
    for paper in [false, true] {
        let mut s = typing(false);
        s.ui.font.as_mut().unwrap().glyphs[0x20] = [0; 8];
        if paper {
            let bmp = u6_formats::bitmap::Bitmap {
                width: 320,
                height: 200,
                pixels: vec![PAPER; 320 * 200],
            };
            let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
            s.ui.paper = Some(text::Paper::drawn(&identity, &bmp));
        }
        let shots: Rc<RefCell<Vec<Shot>>> = Rc::default();
        let kept = Rc::clone(&shots);
        let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
            let shot = |what: &'static str, (col, row): (u16, u16)| {
                let at = (usize::from(col) * 8, usize::from(row) * 8);
                let srcs = io.draw(|fb| {
                    (0..64)
                        .map(|k| fb.src_at(at.0 + k % 8, at.1 + k / 8).unwrap())
                        .collect()
                });
                kept.borrow_mut().push((what, (col, row), srcs));
            };
            {
                let p = st.borrow().ui.paper.clone();
                io.draw(|fb| text::fill_rect(fb, 0, 0, 319, 199, PAPER, p.as_ref()));
            }
            let echo = |c: u16| keyread::echo(&mut st.borrow_mut(), &io, c);
            echo(0x20);
            shot("a typed space", (22, 14));
            echo(8);
            shot("Backspace over a typed space", (22, 14));
            echo(u16::from(b'a'));
            shot("a typed letter", (22, 14));
            echo(8);
            shot("Backspace over a typed letter", (22, 14));
            // The row's last cell: the echo wraps to the next row; Backspace comes back across.
            st.borrow_mut().ui.text.win_mut().col = 16;
            echo(u16::from(b'a'));
            shot("a typed letter at the row's end", (38, 14));
            assert_eq!(
                st.borrow().ui.text.win().row,
                1,
                "precondition: the echo wrapped"
            );
            echo(8);
            shot("Backspace back across the row's end", (38, 14));
            io.present();
        });
        e.enable_src_plane();
        e.tick(&[], 16);
        assert!(e.finished());
        let shots = shots.borrow();
        assert_eq!(shots.len(), 6);
        let on = if paper { FLAG_PAPER } else { 0 };
        for (i, (what, at, srcs)) in shots.iter().enumerate() {
            let typed = i % 2 == 0;
            let (code, rows) = match (typed, i) {
                (true, 0) => (0x20, [0u8; 8]),
                (true, _) => (u16::from(b'a'), [b'a'; 8]),
                (false, _) => (0x20, [0u8; 8]),
            };
            for (k, s) in srcs.iter().enumerate() {
                let (u, v) = (k % 8, k / 8);
                let ink = rows[v] & (0x80 >> u) != 0;
                let want = Src {
                    ver: 0,
                    id: code,
                    u: u as u16,
                    v: v as u16,
                    kind: Kind::Glyph,
                    flags: if ink { FLAG_INK } else { 0 } | on | if typed { FLAG_TYPED } else { 0 },
                };
                assert_eq!(
                    *s, want,
                    "paper {paper}: {what} at {at:?}, pixel ({u}, {v})"
                );
            }
        }
    }
}

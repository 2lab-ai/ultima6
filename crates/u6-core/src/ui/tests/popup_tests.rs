//! The context menu drawn in the frame (spec 17 Presentation, unit M3) on synthetic data: its
//! rows and placement, its pixels (paper, ink, the frame's ring, the highlight's mark, the
//! checker of a disabled entry; nothing outside its box), its input (hover, choose, close by the
//! pointer and by keys; what reaches the game) and the engine's screen frame with it.

use super::*;
use crate::engine::{POINTER_LEFT, POINTER_RIGHT, Pointer};
use crate::hd::HdView;
use crate::ui::menu::{self, Entry, GO_HERE, Kind, Menu, Reason, Run, Section};
use crate::ui::popup::{Open, Popup, Row};
use u6_render::hd::Scale;

/// The colour the tests' frames are filled with before the menu is drawn.
const UNDER: u8 = 9;
/// The press the menus here are for (Original cell (5, 5)).
const AT: (i16, i16) = (40, 40);
/// The pressed map cell.
const CELL_PRESSED: (i16, i16) = (10, 12);

fn entry(code: u16, reason: Option<Reason>) -> Entry {
    Entry {
        code,
        reason,
        theft: false,
    }
}

/// A map menu: one section named "ab" with Look and Use (disabled: far), then Go here.
fn menu() -> Menu {
    Menu {
        kind: Kind::Map,
        cell: Some(CELL_PRESSED),
        underlined: None,
        sections: vec![Section {
            object: 300,
            kind: None,
            name: "ab".into(),
            entries: vec![entry(menu::LOOK, None), entry(menu::USE, Some(Reason::Far))],
        }],
        go_here: Some(entry(GO_HERE, None)),
    }
}

/// The synthetic state at the command prompt.
fn prompt() -> State {
    let mut s = state();
    s.main.reading = 1;
    s.ui.input.accept = true;
    s
}

/// A frame of [`UNDER`].
fn under() -> Framebuffer {
    let mut fb = Framebuffer::new();
    for y in 0..u6_render::HEIGHT {
        for x in 0..u6_render::WIDTH {
            fb.set(x, y, UNDER);
        }
    }
    fb
}

/// The middle of row `i`'s label.
fn row_point(o: &Open, i: usize) -> (i16, i16) {
    let (x0, y0, ..) = o.text_rect();
    ((x0 + 12) as i16, (y0 + i as i32 * CELL as i32 + 4) as i16)
}

fn ptr(x: i16, y: i16, buttons: u8) -> InputEvent {
    InputEvent::Pointer { x, y, buttons }
}

/// A popup on, `open` shown.
fn popup_with(open: &Open) -> Popup {
    let mut p = Popup::default();
    p.set_on(true);
    p.show(open.clone());
    p
}

#[test]
fn the_menu_lists_heading_entries_go_here_and_the_note_beside_the_press() {
    let s = prompt();
    let o = Open::new(&menu(), AT, &s, &[]);
    let kinds: Vec<String> = o.rows.iter().map(|r| format!("{r:?}")).collect();
    assert_eq!(o.rows.len(), 5, "{kinds:?}");
    assert!(
        matches!(&o.rows[0], Row::Head(c) if c.len() == 2),
        "the heading first"
    );
    let codes: Vec<u16> = (0..o.rows.len())
        .filter_map(|i| o.item(i).map(|i| i.code))
        .collect();
    assert_eq!(
        codes,
        [menu::LOOK, menu::USE, GO_HERE],
        "the entries, Go here last"
    );
    assert!(
        matches!(o.rows[4], Row::Note),
        "a note row (Use has a reason)"
    );
    assert_eq!(o.hover, 1, "the first entry that can run highlighted");
    // The widest row is the note (12 cells), one ring of ink (no paper), the text area one cell
    // right of and below the pressed cell.
    assert_eq!(o.rings, [INK], "one ring of ink without the paper");
    assert_eq!(o.text, (6, 6, 12, 5));
    assert_eq!(o.text_rect(), (48, 48, 143, 87));
    assert_eq!(o.rect(), (47, 47, 144, 88));
    assert_eq!(o.row_at(60, 60), Some(1));
    assert_eq!(o.row_at(47, 60), None, "the ring is not a row");
    assert!(o.contains(47, 47) && !o.contains(46, 60));
    let v = o.view();
    assert_eq!(
        (v.rect, v.text, v.hover),
        ((47, 47, 144, 88), (48, 48, 143, 87), 1)
    );
    assert_eq!(
        v.rows.iter().map(|r| r.y).collect::<Vec<_>>(),
        [48, 56, 64, 72, 80]
    );
    assert!(!v.rows[2].enabled() && v.rows[2].reason == Some(Reason::Far));
}

#[test]
fn the_underlined_command_comes_first_and_rest_only_there() {
    let s = prompt();
    let mut m = menu();
    m.underlined = Some(menu::USE);
    let o = Open::new(&m, AT, &s, &[]);
    let codes: Vec<u16> = (0..o.rows.len())
        .filter_map(|i| o.item(i).map(|i| i.code))
        .collect();
    assert_eq!(codes, [menu::USE, menu::LOOK, GO_HERE], "Use first, once");
    m.underlined = Some(menu::REST);
    let o = Open::new(&m, AT, &s, &[]);
    let codes: Vec<u16> = (0..o.rows.len())
        .filter_map(|i| o.item(i).map(|i| i.code))
        .collect();
    assert_eq!(codes, [menu::REST, menu::LOOK, menu::USE, GO_HERE]);
}

#[test]
fn near_the_frames_corner_the_menu_flips_and_stays_inside() {
    let s = prompt();
    let at = (
        (u6_render::WIDTH - 10) as i16,
        (u6_render::HEIGHT - 5) as i16,
    );
    let o = Open::new(&menu(), at, &s, &[]);
    let (x0, y0, x1, y1) = o.rect();
    let (tx1, ty1) = (o.text_rect().2, o.text_rect().3);
    assert!(
        tx1 < i32::from(at.0) && ty1 < i32::from(at.1),
        "left of and above the press"
    );
    assert!(x0 >= 0 && y0 >= 0, "inside");
    assert!(
        x1 < u6_render::WIDTH as i32 && y1 < u6_render::HEIGHT as i32,
        "inside"
    );
    assert_eq!(o.text.2, 12, "the whole width");
}

/// Spec 17 Acceptance 8 (synthetic): the text area is paper, ink and the heading's alert colour
/// only; its ring is ink; outside the box the frame is untouched. The heading's glyphs in the
/// alert colour from column 0, the highlight's mark (glyph 0x1a) in column 0, the label from
/// column 1, the disabled label in the checker (half its ink pixels), the note empty while the
/// highlighted entry has none and the reason's words once Use is highlighted.
#[test]
fn the_menu_is_drawn_in_paper_and_ink_inside_its_box_and_nowhere_else() {
    let s = prompt();
    let mut o = Open::new(&menu(), AT, &s, &[]);
    let mut fb = under();
    o.draw(&mut fb, &s);
    let (x0, y0, x1, y1) = o.rect();
    let (tx0, ty0, tx1, ty1) = o.text_rect();
    let mut counts = [0usize; 256];
    for y in 0..u6_render::HEIGHT as i32 {
        for x in 0..u6_render::WIDTH as i32 {
            let c = fb.get(x as usize, y as usize).unwrap_or(0);
            let in_box = (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
            let in_text = (tx0..=tx1).contains(&x) && (ty0..=ty1).contains(&y);
            if !in_box {
                assert_eq!(c, UNDER, "outside the box at ({x}, {y})");
            } else if in_text {
                assert!([PAPER, INK, ALERT].contains(&c), "({x}, {y}): {c}");
                counts[usize::from(c)] += 1;
            } else {
                assert_eq!(c, INK, "the ring at ({x}, {y})");
            }
        }
    }
    assert!(
        counts[usize::from(PAPER)] > counts[usize::from(INK)],
        "mostly paper"
    );
    // The heading: 'a' (rows 0b0110_0001) in the alert colour at column 0.
    let row = |fb: &Framebuffer, x: i32, y: i32| -> Vec<u8> {
        (0..8)
            .map(|dx| fb.get((x + dx) as usize, y as usize).unwrap_or(0))
            .collect()
    };
    let bits = |b: u8, on: u8| -> Vec<u8> {
        (0..8)
            .map(|dx| if b & (0x80 >> dx) != 0 { on } else { PAPER })
            .collect()
    };
    assert_eq!(
        row(&fb, tx0, ty0),
        bits(b'a', ALERT),
        "the heading's first glyph"
    );
    assert_eq!(
        row(&fb, tx0, ty0 + 8),
        bits(0x1a, INK),
        "the highlight's mark"
    );
    assert_eq!(
        row(&fb, tx0 + 8, ty0 + 8),
        bits(b'L', INK),
        "Look's first letter"
    );
    assert_eq!(
        row(&fb, tx0, ty0 + 16),
        bits(0, INK),
        "no mark on another entry"
    );
    // Use, disabled: of 'U' (0b0101_0101) only the pixels with (x + y) even are ink.
    let (ux, uy) = (tx0 + 8, ty0 + 16);
    let mut ink = 0;
    for dy in 0..8 {
        for dx in 0..8 {
            let (x, y) = (ux + dx, uy + dy);
            let c = fb.get(x as usize, y as usize).unwrap_or(0);
            let want = b'U' & (0x80 >> dx) != 0 && (x + y) % 2 == 0;
            assert_eq!(c == INK, want, "the checker at ({x}, {y})");
            ink += usize::from(c == INK);
        }
    }
    assert_eq!(ink, 16, "half the glyph's 32 ink pixels");
    let note_y = ty0 + 4 * 8;
    assert!(
        (tx0..=tx1)
            .all(|x| (0..8).all(|dy| fb.get(x as usize, (note_y + dy) as usize) == Some(PAPER))),
        "the note empty while Look is highlighted"
    );
    o.hover = 2;
    let mut fb = under();
    o.draw(&mut fb, &s);
    assert_eq!(
        row(&fb, tx0, note_y),
        bits(b'T', INK),
        "the reason's words in the note"
    );
}

/// Spec 17 Presentation, Input: a move over an entry highlights it; Up and Down move over the
/// entries, wrapping, disabled ones included; a press on a disabled entry, a heading or the ring
/// does nothing; none of it reaches the game.
#[test]
fn moves_and_arrows_highlight_and_reach_nothing() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    let o = Open::new(&menu(), AT, &s, &[]);
    let mut p = popup_with(&o);
    let v0 = p.version();
    let (gx, gy) = row_point(&o, 3);
    assert!(
        p.input(&s, &io, &[ptr(gx, gy, 0)], true).is_empty(),
        "a move: the menu's"
    );
    assert_eq!(p.open().map(|o| o.hover), Some(3), "Go here highlighted");
    assert!(p.version() > v0, "drawn again");
    let keys = |p: &mut Popup, k: Key| p.input(&s, &io, &[InputEvent::Key(k)], true);
    assert!(keys(&mut p, Key::Down).is_empty());
    assert_eq!(p.open().map(|o| o.hover), Some(1), "wrapped to Look");
    assert!(keys(&mut p, Key::Up).is_empty());
    assert_eq!(p.open().map(|o| o.hover), Some(3), "back to Go here");
    assert!(keys(&mut p, Key::Up).is_empty());
    assert_eq!(
        p.open().map(|o| o.hover),
        Some(2),
        "Use, disabled, included"
    );
    assert!(keys(&mut p, Key::Enter).is_empty(), "Enter on it: nothing");
    assert!(
        keys(&mut p, Key::Char('u')).is_empty(),
        "its letter: nothing"
    );
    let (ux, uy) = row_point(&o, 2);
    let (hx, hy) = row_point(&o, 0);
    let (rx, ry) = (o.rect().0 as i16, o.rect().1 as i16);
    for (x, y) in [(ux, uy), (hx, hy), (rx, ry)] {
        assert!(
            p.input(&s, &io, &[ptr(x, y, 1)], true).is_empty(),
            "a press there: the menu's"
        );
        assert!(
            p.input(&s, &io, &[ptr(x, y, 0)], true).is_empty(),
            "its release too"
        );
        assert!(p.open().is_some(), "still open");
    }
    assert!(
        p.input(&s, &io, &[InputEvent::Text('x')], true).is_empty(),
        "text: the menu's"
    );
    assert!(p.run().is_none(), "nothing runs");
}

/// Spec 17 Running a choice: a left press on Look closes the menu and gives the game the
/// pointer back where it is, then Look's icon press; the press's release goes to the game as a
/// move; the target press waits for Look's target read, with the queues quiet, and then follows
/// that tick's events.
#[test]
fn a_left_press_on_look_runs_it_through_the_target_read() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    let o = Open::new(&menu(), AT, &s, &[]);
    let mut p = popup_with(&o);
    let (lx, ly) = row_point(&o, 1);
    let out = p.input(&s, &io, &[ptr(lx, ly, 1)], true);
    let icon = menu::icon_press(menu::LOOK);
    assert_eq!(out, [ptr(lx, ly, 0), icon[0], icon[1]]);
    assert!(p.open().is_none(), "closed");
    assert_eq!(p.run().map(|r| (r.code, r.at)), Some((menu::LOOK, AT)));
    assert_eq!(
        p.input(&s, &io, &[ptr(lx, ly, 0)], true),
        [ptr(lx, ly, 0)],
        "the release: a move"
    );
    assert!(p.pointer().is_none(), "the game's pointer again");
    let mut reads = s.clone();
    let i = &mut reads.ui.input;
    (i.target_pending, i.target_command, i.accept, i.in_wait) = (1, menu::LOOK, true, true);
    let key = InputEvent::Key(Key::Char('x'));
    assert_eq!(
        p.input(&reads, &io, &[key], true),
        [key],
        "a key queued: held"
    );
    assert!(p.run().is_some());
    assert!(
        p.input(&reads, &io, &[], false).is_empty(),
        "the game's queue busy: held"
    );
    let press = menu::left_press(AT.0, AT.1);
    assert_eq!(
        p.input(&reads, &io, &[ptr(5, 5, 0)], true),
        [ptr(5, 5, 0), press[0], press[1]]
    );
    assert!(p.run().is_none(), "the run over");
}

/// Spec 17 Presentation, Input: Escape closes the menu and its repeats are the menu's up to its
/// release (a held Escape passes no turn); a right press and a left press outside close it, the
/// game's pointer brought where the pointer is, each press the menu's to its release; a
/// command's letter chooses its entry; Enter on Go here starts unit M4's travel to the cell.
#[test]
fn escape_a_right_press_a_press_outside_a_letter_and_go_here() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    let o = Open::new(&menu(), AT, &s, &[]);
    let esc = Key::Escape;
    let mut p = popup_with(&o);
    assert!(
        p.input(&s, &io, &[InputEvent::Key(esc)], true).is_empty(),
        "Escape: the menu's"
    );
    assert!(p.open().is_none(), "closed");
    assert!(
        p.input(&s, &io, &[InputEvent::Repeat(esc)], true)
            .is_empty(),
        "its repeat too"
    );
    let up = InputEvent::Release(esc);
    assert!(
        p.input(&s, &io, &[up], true).is_empty(),
        "its release too (round 2: no key of Escape's reaches the game)"
    );
    let again = InputEvent::Key(esc);
    assert_eq!(
        p.input(&s, &io, &[again], true),
        [again],
        "a new Escape: the game's"
    );

    let (ox, oy) = (200, 150);
    assert!(!o.contains(ox, oy));
    for buttons in [POINTER_RIGHT, POINTER_LEFT] {
        let mut p = popup_with(&o);
        let (x, y) = if buttons == POINTER_RIGHT {
            row_point(&o, 1)
        } else {
            (ox, oy)
        };
        assert_eq!(
            p.input(&s, &io, &[ptr(x, y, buttons)], true),
            [ptr(x, y, 0)]
        );
        assert!(
            p.open().is_none() && p.run().is_none(),
            "{buttons}: closed, nothing runs"
        );
        assert!(
            p.input(&s, &io, &[ptr(x + 3, y, buttons)], true).is_empty(),
            "held: the menu's"
        );
        assert_eq!(
            p.input(&s, &io, &[ptr(x + 3, y, 0)], true),
            [ptr(x + 3, y, 0)]
        );
    }

    let mut p = popup_with(&o);
    let out = p.input(&s, &io, &[InputEvent::Key(Key::Char('L'))], true);
    assert_eq!(out, menu::icon_press(menu::LOOK), "L: Look");
    assert_eq!(p.run().map(|r| r.code), Some(menu::LOOK));

    let mut p = popup_with(&o);
    let (gx, gy) = row_point(&o, 3);
    assert!(p.input(&s, &io, &[ptr(gx, gy, 0)], true).is_empty());
    assert_eq!(
        p.input(&s, &io, &[InputEvent::Key(Key::Enter)], true),
        [ptr(gx, gy, 0)]
    );
    assert_eq!(
        p.take_go_here(),
        Some(CELL_PRESSED),
        "Go here: travel to the cell"
    );
    assert_eq!(p.take_go_here(), None, "handed out once");
    assert!(p.run().is_none() && p.open().is_none());
}

/// The menu closes when the prompt goes, and when it is turned off (a run dropped too).
#[test]
fn the_menu_closes_when_the_prompt_goes_or_it_is_turned_off() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    let o = Open::new(&menu(), AT, &s, &[]);
    let mut p = popup_with(&o);
    p.after_poll(&s);
    assert!(p.open().is_some(), "the prompt up: open");
    let mut busy = s.clone();
    busy.main.reading = 0;
    p.after_poll(&busy);
    assert!(p.open().is_none(), "the prompt gone: closed");
    let mut p = popup_with(&o);
    let (lx, ly) = row_point(&o, 1);
    p.input(&s, &io, &[ptr(lx, ly, 1)], true);
    assert!(p.run().is_some());
    p.set_on(false);
    assert!(
        p.run().is_none() && p.open().is_none() && !p.on(),
        "off: nothing left"
    );
    let ev = ptr(lx, ly, 2);
    assert_eq!(Run::start(&s, menu::REST, AT), None, "Rest has no target");
    let mut off = Popup::default();
    assert_eq!(
        off.input(&s, &io, &[ev], true),
        [ev],
        "off: a right press is the game's"
    );
}

/// Spec 17 Presentation, Where it is drawn: the engine draws the open menu into its screen frame
/// over the published frame, which stays as the game drew it; the shown frame and the HD frame
/// are made from the screen frame; the game's pointer is not moved by the menu's events; closed,
/// the screen frame is the published frame again. The versions move for each change of the menu.
#[test]
fn the_engine_draws_the_menu_over_the_published_frame_only() {
    let s = prompt();
    let mut e = Engine::with_game(s.clone(), Files::default(), |_st, io| async move {
        io.draw(|fb| {
            for y in 0..u6_render::HEIGHT {
                for x in 0..u6_render::WIDTH {
                    fb.set(x, y, UNDER + ((x / 16 + y / 16) % 2) as u8);
                }
            }
        });
        io.present();
        loop {
            io.vblank().await;
        }
    });
    e.enable_src_plane();
    e.set_context_menu(true);
    e.tick(&[], 16);
    e.tick(&[], 16);
    let same = |e: &Engine| e.screen_frame().pixels() == e.published_frame().pixels();
    assert!(same(&e), "no menu: the screen frame is the published one");
    assert!(e.context_menu().is_none());
    let blank = Framebuffer::new();
    let mut hd = HdView::new();
    hd.set(true, Scale::X2);
    hd.present(Some(&e), &blank);
    let (published, screen, shown) = (e.published_version(), e.screen_version(), e.frame_version());
    let o = Open::new(&menu(), AT, &s, &[]);
    e.popup_mut().show(o.clone());
    e.tick(&[], 16);
    assert_eq!(
        e.published_version(),
        published,
        "the published frame as it was"
    );
    assert!(
        e.screen_version() > screen && e.frame_version() > shown,
        "composed again"
    );
    assert!(hd.present(Some(&e), &blank), "the HD frame composed again");
    let (x0, y0, x1, y1) = o.rect();
    let (mut inside, mut differ) = (0, 0);
    for y in 0..u6_render::HEIGHT {
        for x in 0..u6_render::WIDTH {
            let (a, b) = (e.screen_frame().get(x, y), e.published_frame().get(x, y));
            let (xi, yi) = (x as i32, y as i32);
            if (x0..=x1).contains(&xi) && (y0..=y1).contains(&yi) {
                inside += 1;
                differ += usize::from(a != b);
            } else {
                assert_eq!(a, b, "outside the box at ({x}, {y})");
            }
        }
    }
    assert!(differ * 2 > inside, "the box drawn: {differ} of {inside}");
    assert!(
        e.frame().pixels() == e.screen_frame().pixels(),
        "no sprite: shown = screen"
    );
    assert_eq!(e.context_menu(), Some(o.view()));
    let game_pointer = e.io().handler_pointer();
    let (gx, gy) = row_point(&o, 3);
    let v = e.screen_version();
    e.tick(&[ptr(gx, gy, 0)], 16);
    assert_eq!(e.context_menu().map(|v| v.hover), Some(3), "hovered");
    assert!(e.screen_version() > v, "drawn again");
    assert_eq!(
        e.io().handler_pointer(),
        game_pointer,
        "the game's pointer not moved"
    );
    let v = e.screen_version();
    e.tick(&[InputEvent::Key(Key::Escape)], 16);
    assert!(
        e.context_menu().is_none() && same(&e),
        "closed: the published frame again"
    );
    assert!(e.screen_version() > v);
    assert_eq!(
        e.io().handler_pointer(),
        Pointer {
            x: gx,
            y: gy,
            buttons: 0
        },
        "the pointer back"
    );
    assert_eq!(e.published_version(), published);
}

/// Spec 17 Presentation, Words: with the Korean language set (as the Korean pack sets it) the
/// menu is laid out and drawn from the Korean words of the one table: the text area is as wide
/// as the widest Korean row (Go here's 6 cells and the mark's column, 7, where the English rows
/// make it 12), and each label's syllables are drawn from the Hangul table (here every syllable a
/// full block: a cell of ink), its spaces with the game font's space.
#[test]
fn korean_words_are_laid_out_and_drawn_with_the_hangul_glyphs() {
    let mut s = prompt();
    s.ui.text.language = crate::ui::wide::Language::Korean;
    let glyphs = vec![[0xff; 8]; u6_formats::font::HANGUL_GLYPHS];
    s.ui.hangul = Some(Rc::new(
        u6_formats::font::HangulFont::from_glyphs(glyphs).expect("a table"),
    ));
    let o = Open::new(&menu(), AT, &s, &[]);
    assert!(o.korean);
    assert_eq!(o.text.2, 7, "the widest Korean row");
    let mut fb = under();
    o.draw(&mut fb, &s);
    let (tx0, ty0, ..) = o.text_rect();
    let cell = |fb: &Framebuffer, col: i32, row: i32| -> Vec<u8> {
        let (x, y) = (tx0 + col * 8, ty0 + row * 8);
        (0..64)
            .map(|k| {
                fb.get((x + k % 8) as usize, (y + k / 8) as usize)
                    .unwrap_or(0)
            })
            .collect()
    };
    // Go here (row 3): six cells from column 1, a syllable, a syllable, a syllable, a space, two
    // syllables.
    let ink = vec![INK; 64];
    // The tests' font draws glyph c as rows of c: the space (0x20) is one column of ink.
    let space: Vec<u8> = (0..64)
        .map(|k| {
            if 0x20u8 & (0x80 >> (k % 8)) != 0 {
                INK
            } else {
                PAPER
            }
        })
        .collect();
    let shown: Vec<bool> = (1..=6).map(|c| cell(&fb, c, 3) == ink).collect();
    assert_eq!(
        shown,
        [true, true, true, false, true, true],
        "Go here's syllables in ink"
    );
    assert_eq!(cell(&fb, 4, 3), space, "its space, the game font's glyph");
    // Look (row 1): three syllables, highlighted (the mark in column 0).
    assert!((1..=3).all(|c| cell(&fb, c, 1) == ink), "Look's syllables");
}

/// Spec 17 Presentation, Where it is placed: a menu keeps off the frame rects the platform shows
/// something else over (Wide's page windows): with the place right of and below the press hidden
/// it takes the next side; with every side beside the press hidden it takes the place in the
/// bounds that shows it whole; with nothing hidden, the first place.
#[test]
fn the_menu_keeps_off_what_the_platform_hides() {
    let s = prompt();
    // A press mid-frame, so every side has room.
    let at = (160, 96);
    let first = Open::new(&menu(), at, &s, &[]);
    let right_below = [first.rect()];
    let o = Open::new(&menu(), at, &s, &right_below);
    let (a0, b0, a1, b1) = o.rect();
    let (x0, y0, x1, y1) = first.rect();
    let text_right = o.text_rect().2;
    assert!(
        text_right < i32::from(at.0),
        "its text left of the press: {:?}",
        o.rect()
    );
    assert_eq!((b0, b1), (y0, y1), "below it still");
    assert_eq!((a1 - a0, b1 - b0), (x1 - x0, y1 - y0), "the same size");
    // Everything within 120 pixels of a press near the corner hidden: the menu lies past it,
    // whole, inside the frame.
    let (px, py) = (i32::from(AT.0), i32::from(AT.1));
    let near = [(px - 120, py - 120, px + 120, py + 120)];
    let o = Open::new(&menu(), AT, &s, &near);
    let (a0, b0, a1, b1) = o.rect();
    let meets = a0 <= near[0].2 && a1 >= near[0].0 && b0 <= near[0].3 && b1 >= near[0].1;
    assert!(!meets, "off the hidden rect: {:?}", o.rect());
    assert!(a0 >= 0 && b0 >= 0 && a1 < u6_render::WIDTH as i32 && b1 < u6_render::HEIGHT as i32);
}

/// A map menu of `entries` (all can run), Go here last.
fn menu_of(entries: &[u16]) -> Menu {
    Menu {
        kind: Kind::Map,
        cell: Some(CELL_PRESSED),
        underlined: None,
        sections: vec![Section {
            object: 300,
            kind: None,
            name: "ab".into(),
            entries: entries.iter().map(|&c| entry(c, None)).collect(),
        }],
        go_here: Some(entry(GO_HERE, None)),
    }
}

/// Whether `out` gives the game a button (a press, or a release after one).
fn presses(out: &[InputEvent]) -> usize {
    out.iter()
        .filter(|e| matches!(e, InputEvent::Pointer { buttons, .. } if buttons & 3 != 0))
        .count()
}

/// Spec 17 Presentation, Input (round 2, opus MUST 1, gpt e MUST 2): every key the menu took is
/// the menu's to its own release, so a held Down whose highlight move the menu took passes no
/// turn when Escape (or Enter, choosing) closes the menu while it is held: its repeats and its
/// release, and Escape's own, reach no game. A key whose press was the game's gets its release.
#[test]
fn every_key_the_menu_took_keeps_its_repeats_to_its_own_release() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    for close in [Key::Escape, Key::Enter] {
        let o = Open::new(&menu_of(&[menu::LOOK]), AT, &s, &[]);
        let mut p = popup_with(&o);
        let feed = |p: &mut Popup, ev: InputEvent| p.input(&s, &io, &[ev], true);
        assert!(
            feed(&mut p, InputEvent::Key(Key::Down)).is_empty(),
            "{close:?}: Down"
        );
        assert!(
            feed(&mut p, InputEvent::Key(close)).is_empty(),
            "{close:?}: taken"
        );
        assert!(p.open().is_none(), "{close:?}: closed");
        if close == Key::Enter {
            assert_eq!(p.take_go_here(), Some(CELL_PRESSED), "Enter chose Go here");
        }
        for _ in 0..3 {
            assert!(
                feed(&mut p, InputEvent::Repeat(Key::Down)).is_empty(),
                "{close:?}: the held Down's repeat reaches no game"
            );
        }
        assert!(
            feed(&mut p, InputEvent::Repeat(close)).is_empty(),
            "{close:?}: its own repeat"
        );
        for k in [Key::Down, close] {
            let up = InputEvent::Release(k);
            assert!(
                feed(&mut p, up).is_empty(),
                "{close:?}: {k:?} let go: the menu's too"
            );
        }
        let after = InputEvent::Repeat(Key::Down);
        assert_eq!(
            feed(&mut p, after),
            [after],
            "{close:?}: then the game's again"
        );
    }
    // A key pressed for the game before the menu opened: its repeats while open are the menu's,
    // its release the game's (its press was).
    let o = Open::new(&menu_of(&[menu::LOOK]), AT, &s, &[]);
    let mut p = popup_with(&o);
    let feed = |p: &mut Popup, ev: InputEvent| p.input(&s, &io, &[ev], true);
    assert!(
        feed(&mut p, InputEvent::Repeat(Key::Left)).is_empty(),
        "the held Left's repeat"
    );
    assert!(
        feed(&mut p, InputEvent::Key(Key::Escape)).is_empty(),
        "closed"
    );
    assert!(
        feed(&mut p, InputEvent::Repeat(Key::Left)).is_empty(),
        "still the menu's"
    );
    let up = InputEvent::Release(Key::Left);
    assert_eq!(feed(&mut p, up), [up], "its release: the game's");
    // A key the menu took, pressed again after the release was missed: the game's from that press.
    let press = InputEvent::Key(Key::Escape);
    assert_eq!(
        feed(&mut p, press),
        [press],
        "a new press of Escape: the game's"
    );
    let rep = InputEvent::Repeat(Key::Escape);
    assert_eq!(feed(&mut p, rep), [rep], "and its repeats");
}

/// Spec 17 Presentation, Input (round 2, gpt a2 MUST 1): a double click on an entry chooses it
/// once and gives the game no press, in one batch and across ticks.
#[test]
fn a_double_click_on_an_entry_chooses_it_once_and_leaks_no_press() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    let o = Open::new(&menu_of(&[menu::LOOK]), AT, &s, &[]);
    let go = (0..o.rows.len())
        .find(|&i| o.item(i).is_some_and(|it| it.code == GO_HERE))
        .expect("Go here");
    let (gx, gy) = row_point(&o, go);
    let clicks = [
        ptr(gx, gy, 1),
        ptr(gx, gy, 0),
        ptr(gx, gy, 1),
        ptr(gx, gy, 0),
    ];
    let mut p = popup_with(&o);
    let out = p.input(&s, &io, &clicks, true);
    assert_eq!(presses(&out), 0, "one batch: {out:?}");
    assert_eq!(
        (p.take_go_here(), p.take_go_here()),
        (Some(CELL_PRESSED), None),
        "one batch: chosen once"
    );
    let mut p = popup_with(&o);
    let mut all = Vec::new();
    for ev in clicks {
        all.extend(p.input(&s, &io, &[ev], true));
    }
    assert_eq!(presses(&all), 0, "across ticks: {all:?}");
    assert_eq!(
        (p.take_go_here(), p.take_go_here()),
        (Some(CELL_PRESSED), None),
        "across ticks: chosen once"
    );
    // Unit M4's merge contract (spec 21): a travel ends on an event that sets a button the
    // engine's handler lacks, so the choosing press's held moves are the menu's and its release
    // reaches the game as a move with no button.
    let mut p = popup_with(&o);
    let buttoned = |out: &[InputEvent]| {
        out.iter()
            .filter(|e| matches!(e, InputEvent::Pointer { buttons, .. } if buttons & 3 != 0))
            .count()
    };
    let out = p.input(&s, &io, &[ptr(gx, gy, 1)], true);
    assert_eq!(buttoned(&out), 0, "the choosing press: {out:?}");
    for (dx, dy) in [(3, 0), (9, 2), (40, -20)] {
        let out = p.input(&s, &io, &[ptr(gx + dx, gy + dy, 1)], true);
        assert!(
            out.is_empty(),
            "a held move ({dx}, {dy}) is the menu's: {out:?}"
        );
    }
    let out = p.input(&s, &io, &[ptr(gx + 40, gy - 20, 0)], true);
    assert_eq!(
        out,
        [ptr(gx + 40, gy - 20, 0)],
        "its release: a move with no button"
    );
    assert_eq!(p.take_go_here(), Some(CELL_PRESSED), "held: chosen once");
}

/// Spec 21's merge contract with the menu: a travel ends when the menu took new input, and the
/// tail of the gesture that chose Go here is not new (its press's held moves and release, a
/// double click on the entry, the choosing key's repeats and release); a press while a chosen
/// command waits is.
#[test]
fn the_tail_of_a_choice_is_not_new_input() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    let o = Open::new(&menu_of(&[menu::LOOK]), AT, &s, &[]);
    let go = (0..o.rows.len())
        .find(|&i| o.item(i).is_some_and(|it| it.code == GO_HERE))
        .expect("Go here");
    let (gx, gy) = row_point(&o, go);
    let fed = |p: &mut Popup, st: &State, ev: InputEvent| {
        p.input(st, &io, &[ev], true);
        p.took_new_input()
    };
    let mut p = popup_with(&o);
    assert!(fed(&mut p, &s, ptr(gx, gy, 0)), "a move while open");
    assert!(fed(&mut p, &s, ptr(gx, gy, 1)), "the choosing press");
    assert_eq!(p.take_go_here(), Some(CELL_PRESSED), "Go here chosen");
    let tail = [
        ptr(gx + 5, gy, 1),
        ptr(gx + 5, gy, 0),
        ptr(gx + 5, gy, 1),
        ptr(gx + 5, gy, 0),
    ];
    for (i, ev) in tail.into_iter().enumerate() {
        assert!(
            !fed(&mut p, &s, ev),
            "tail {i}: a held move, the release, a double click"
        );
    }
    assert!(
        !fed(&mut p, &s, ptr(gx + 6, gy, 0)),
        "then a move of the game's"
    );
    let mut p = popup_with(&o);
    assert!(
        fed(&mut p, &s, InputEvent::Key(Key::Down)),
        "Down while open"
    );
    assert!(
        fed(&mut p, &s, InputEvent::Key(Key::Enter)),
        "Enter choosing"
    );
    assert_eq!(p.take_go_here(), Some(CELL_PRESSED), "Enter chose Go here");
    for ev in [
        InputEvent::Repeat(Key::Enter),
        InputEvent::Repeat(Key::Down),
        InputEvent::Release(Key::Down),
        InputEvent::Release(Key::Enter),
    ] {
        assert!(!fed(&mut p, &s, ev), "the keys' tail: {ev:?}");
    }
    let o = Open::new(&menu_of(&[menu::ATTACK]), AT, &s, &[]);
    let row = (0..o.rows.len())
        .find(|&i| o.item(i).is_some_and(|it| it.code == menu::ATTACK))
        .expect("Attack");
    let (x, y) = row_point(&o, row);
    let mut p = popup_with(&o);
    assert!(fed(&mut p, &s, ptr(x, y, 1)), "choosing Attack");
    assert!(
        !fed(&mut p, &s, ptr(x, y, 0)),
        "its release: the game's move"
    );
    let mut reads = s.clone();
    let i = &mut reads.ui.input;
    (i.target_pending, i.target_command, i.accept, i.in_wait) = (1, menu::ATTACK, true, true);
    assert!(p.run().is_some(), "Attack waits for its target");
    assert!(
        fed(&mut p, &reads, ptr(200, 150, POINTER_LEFT)),
        "a press while it waits: new"
    );
}

/// Spec 17 Running a choice (round 2, opus SHOULD 1): while the chosen command waits for its
/// target read, the user's presses (left or right) and their releases are the menu's, so none of
/// them becomes the command's target; the target press is the menu's own, at the pressed pixel.
#[test]
fn a_click_while_a_chosen_command_waits_is_not_its_target() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    for code in [menu::ATTACK, menu::USE] {
        for buttons in [POINTER_LEFT, POINTER_RIGHT] {
            let o = Open::new(&menu_of(&[code]), AT, &s, &[]);
            let row = (0..o.rows.len())
                .find(|&i| o.item(i).is_some_and(|it| it.code == code))
                .expect("the command");
            let (x, y) = row_point(&o, row);
            let mut p = popup_with(&o);
            let out = p.input(&s, &io, &[ptr(x, y, 1)], true);
            assert_eq!(
                &out[1..],
                menu::icon_press(code),
                "{code:#x}: its icon press"
            );
            let out = p.input(&s, &io, &[ptr(x, y, 0)], true);
            assert_eq!(
                out,
                [ptr(x, y, 0)],
                "{code:#x}: the choosing press's release: a move"
            );
            assert!(p.run().is_some(), "{code:#x}: held");
            let mut reads = s.clone();
            let i = &mut reads.ui.input;
            (i.target_pending, i.target_command, i.accept, i.in_wait) = (1, code, true, true);
            let out = p.input(&reads, &io, &[ptr(200, 150, buttons)], true);
            assert!(
                out.is_empty(),
                "{code:#x} {buttons}: the press is the menu's: {out:?}"
            );
            assert!(
                p.run().is_some(),
                "{code:#x} {buttons}: still held (a press queued)"
            );
            let out = p.input(&reads, &io, &[ptr(201, 150, 0)], true);
            assert_eq!(
                out,
                menu::left_press(AT.0, AT.1),
                "{code:#x} {buttons}: its release is the menu's; the target press is the menu's own"
            );
            assert!(p.run().is_none(), "{code:#x} {buttons}: the run over");
            let next = ptr(202, 150, 0);
            assert_eq!(
                p.input(&s, &io, &[next], true),
                [next],
                "then the game's again"
            );
        }
    }
}

/// An engine whose game draws a checkered frame once and waits, its source plane on and the
/// menu on; ticked twice (published).
fn checkered_engine(s: &State) -> Engine {
    let mut e = Engine::with_game(s.clone(), Files::default(), |_st, io| async move {
        io.draw(|fb| {
            for (i, c) in fb.palette.iter_mut().enumerate() {
                *c = u6_render::Rgb::new(i as u8, (i * 7) as u8, (i * 13) as u8);
            }
            for y in 0..u6_render::HEIGHT {
                for x in 0..u6_render::WIDTH {
                    fb.set(x, y, UNDER + ((x / 16 + y / 16) % 2) as u8);
                }
            }
        });
        io.present();
        loop {
            io.vblank().await;
        }
    });
    e.enable_src_plane();
    e.set_context_menu(true);
    e.tick(&[], 16);
    e.tick(&[], 16);
    e
}

/// The shown frame's colours, palette and source plane.
fn shown_frame(e: &Engine) -> (Vec<u8>, Vec<u6_render::Rgb>, Option<Vec<u6_render::Src>>) {
    let f = e.frame();
    (
        f.pixels().to_vec(),
        f.palette.to_vec(),
        f.src().map(<[_]>::to_vec),
    )
}

/// Spec 17 Presentation (round 2, opus MUST 2): in the HD font mode the menu's glyphs are listed
/// for the HD font: the heading's, the enabled entry's, and the disabled entry's flagged
/// half-toned (the page draws its HD glyph so); with the mode off the HD frame shows the classic
/// checker. Closed and presented, the HD frame is the one before it opened, byte for byte (gpt f2
/// MUST 1), and so is the shown frame with its palette and source plane (gpt f2 SHOULD 2).
#[test]
fn the_hd_font_lists_the_menus_cells_the_disabled_row_half_toned() {
    let s = prompt();
    let mut e = checkered_engine(&s);
    let blank = Framebuffer::new();
    let mut hd = HdView::new();
    hd.set(true, Scale::X2);
    hd.set_font(true);
    hd.present(Some(&e), &blank);
    let (hd_before, shown_before) = (hd.rgba().to_vec(), shown_frame(&e));
    let o = Open::new(&menu(), AT, &s, &[]);
    e.popup_mut().show(o.clone());
    e.tick(&[], 16);
    assert!(hd.present(Some(&e), &blank), "composed with the menu");
    assert_ne!(
        hd.rgba(),
        hd_before.as_slice(),
        "the open menu shows in the HD frame"
    );
    let (col0, row0, ..) = o.text;
    let listed = |row: i32, word: &[u8], col: i32| -> Vec<(u16, bool)> {
        word.iter()
            .enumerate()
            .filter_map(|(k, _)| {
                let (x, y) = (
                    ((col0 + col + k as i32) * 8) as u16,
                    ((row0 + row) * 8) as u16,
                );
                hd.glyphs()
                    .iter()
                    .find(|g| (g.x, g.y) == (x, y))
                    .map(|g| (g.code, g.halftone))
            })
            .collect()
    };
    let want = |word: &[u8], half: bool| -> Vec<(u16, bool)> {
        word.iter().map(|&c| (u16::from(c), half)).collect()
    };
    assert_eq!(listed(0, b"ab", 0), want(b"ab", false), "the heading");
    assert_eq!(listed(1, b"Look", 1), want(b"Look", false), "Look, enabled");
    assert_eq!(
        listed(2, b"Use", 1),
        want(b"Use", true),
        "Use, disabled: half-toned"
    );
    // The tests' font's space has ink (glyph c is rows of c): neither a letter nor a whole space.
    assert_eq!(listed(3, b"Go here", 1).len(), 6, "Go here's letters");
    let halftoned = hd.glyphs().iter().filter(|g| g.halftone).count();
    assert_eq!(halftoned, 3, "only the disabled row's cells");
    hd.set_font(false);
    hd.present(Some(&e), &blank);
    assert!(hd.glyphs().is_empty(), "the pixel font: nothing listed");
    // The classic checker in the screen frame: of 'U' only the pixels with (x + y) even are ink.
    let (ux, uy) = ((col0 + 1) * 8, (row0 + 2) * 8);
    let ink = (0..64)
        .filter(|k| {
            let (x, y) = (ux + k % 8, uy + k / 8);
            let want = b'U' & (0x80 >> (k % 8)) != 0 && (x + y) % 2 == 0;
            assert_eq!(
                e.screen_frame().get(x as usize, y as usize) == Some(INK),
                want
            );
            want
        })
        .count();
    assert_eq!(ink, 16, "the classic checker");
    hd.set_font(true);
    e.tick(&[InputEvent::Key(Key::Escape)], 16);
    assert!(e.context_menu().is_none(), "closed");
    hd.present(Some(&e), &blank);
    assert_eq!(
        hd.rgba(),
        hd_before.as_slice(),
        "the HD frame as before it opened"
    );
    assert!(
        hd.glyphs().iter().all(|g| !g.halftone),
        "nothing half-toned left"
    );
    assert!(
        shown_frame(&e) == shown_before,
        "the shown frame, palette and plane as before"
    );
}

/// Spec 11 Design 7 for K3 (round 2, opus SHOULD 6): `pointer_at` is the menu's pointer while the
/// menu takes the pointer's events, the input handler's otherwise.
#[test]
fn pointer_at_is_the_menus_pointer_while_it_takes_it() {
    let s = prompt();
    let mut e = checkered_engine(&s);
    assert_eq!(
        e.pointer_at(),
        e.io().handler_pointer(),
        "no menu: the handler's"
    );
    let o = Open::new(&menu(), AT, &s, &[]);
    e.popup_mut().show(o.clone());
    let (gx, gy) = row_point(&o, 3);
    e.tick(&[ptr(gx, gy, 0)], 16);
    let menus = Pointer {
        x: gx,
        y: gy,
        buttons: 0,
    };
    assert_eq!(e.pointer_at(), menus, "the menu's");
    assert_ne!(
        e.io().handler_pointer(),
        menus,
        "the game's sample not moved"
    );
    e.tick(&[InputEvent::Key(Key::Escape)], 16);
    assert_eq!(
        e.pointer_at(),
        e.io().handler_pointer(),
        "closed: the handler's again"
    );
}

/// Spec 17 (round 2, opus SHOULD 2): turned off, the menu forgets the pointer, the press and the
/// keys it took; turned on again it takes none of their events.
#[test]
fn turned_off_it_forgets_what_it_took() {
    let (s, io) = (prompt(), Io::new(Files::default()));
    let o = Open::new(&menu(), AT, &s, &[]);
    let mut p = popup_with(&o);
    let (x, y) = row_point(&o, 3);
    assert!(p.input(&s, &io, &[ptr(x, y, POINTER_RIGHT)], true).len() <= 1);
    let o2 = Open::new(&menu(), AT, &s, &[]);
    p.show(o2);
    assert!(
        p.input(&s, &io, &[InputEvent::Key(Key::Down)], true)
            .is_empty()
    );
    p.set_on(false);
    assert!(p.pointer().is_none(), "off: no pointer of the menu's");
    p.set_on(true);
    let rep = InputEvent::Repeat(Key::Down);
    assert_eq!(
        p.input(&s, &io, &[rep], true),
        [rep],
        "the held key: the game's again"
    );
    let up = ptr(x, y, 0);
    assert_eq!(
        p.input(&s, &io, &[up], true),
        [up],
        "the held button's release: the game's"
    );
    assert!(p.pointer().is_none());
}

/// Spec 17 Presentation, Where it is placed (round 2, gpt a2 SHOULD, opus NIT): an open menu is
/// laid out again when what the platform hides changes (here its own box hidden: it moves off
/// it) and when the language changes, the same entry highlighted; the engine draws it again at
/// once.
#[test]
fn an_open_menu_is_laid_out_again_for_new_hidden_rects_and_language() {
    let s = prompt();
    let at = (160, 96);
    let mut e = checkered_engine(&s);
    let o = Open::new(&menu(), at, &s, &[]);
    e.popup_mut().show(o.clone());
    e.tick(&[InputEvent::Key(Key::Up)], 16);
    let hover = e.context_menu().map(|v| v.hover).expect("open");
    let (v, screen) = (e.popup_mut().version(), e.screen_version());
    e.set_context_menu_hidden(vec![o.rect()]);
    let moved = e.context_menu().expect("still open");
    assert_ne!(moved.rect, o.view().rect, "moved off what is hidden");
    assert_eq!(moved.hover, hover, "the same entry highlighted");
    assert!(e.popup_mut().version() > v, "drawn again");
    assert!(e.screen_version() > screen, "at once, no tick");
    let mut korean = s.clone();
    korean.ui.text.language = crate::ui::wide::Language::Korean;
    let mut p = popup_with(&o);
    p.after_poll(&korean);
    let k = p.open().expect("open");
    assert!(k.korean && k.text.2 != o.text.2, "laid out in Korean");
}

/// Spec 17 Presentation (round 2, opus NITs): with two sections the underlined first entry shows
/// its section's name after it, in the alert colour; an entry's theft mark is shown on its row
/// whether it is highlighted or not, and needs no note row.
#[test]
fn the_first_entry_names_its_object_and_a_theft_shows_on_its_row() {
    let s = prompt();
    let m = Menu {
        kind: Kind::Map,
        cell: Some(CELL_PRESSED),
        underlined: Some(menu::USE),
        sections: vec![
            Section {
                object: 300,
                kind: None,
                name: "ab".into(),
                entries: vec![entry(menu::LOOK, None), entry(menu::USE, None)],
            },
            Section {
                object: 301,
                kind: None,
                name: "cd".into(),
                entries: vec![Entry {
                    code: menu::GET,
                    reason: None,
                    theft: true,
                }],
            },
        ],
        go_here: None,
    };
    let o = Open::new(&m, AT, &s, &[]);
    assert_eq!(
        o.named.as_ref().map(|n| n.0.len()),
        Some(2),
        "Use's section named"
    );
    assert!(
        !o.rows.iter().any(|r| matches!(r, Row::Note)),
        "no note row: no reason"
    );
    let get = (0..o.rows.len())
        .find(|&i| o.item(i).is_some_and(|it| it.code == menu::GET))
        .expect("Get");
    assert_ne!(o.hover, get, "Get not highlighted");
    let mut fb = under();
    o.draw(&mut fb, &s);
    let (tx0, ty0, ..) = o.text_rect();
    let alert_in = |row: i32, col: i32| {
        (0..64).any(|k| {
            let (x, y) = (tx0 + col * 8 + k % 8, ty0 + row * 8 + k / 8);
            fb.get(x as usize, y as usize) == Some(ALERT)
        })
    };
    // Row 0: mark, "Use", a space, then the name in the alert colour (column 5).
    assert!(alert_in(0, 5) && !alert_in(0, 1), "the name after Use");
    // Get's row: mark, "Get", a space, then the theft mark from column 5.
    assert!(alert_in(get as i32, 5), "the theft mark, not highlighted");
    assert_eq!(o.text.2, 13, "the widest row: Get and its theft mark");
}

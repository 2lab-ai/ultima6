//! The owner's 2026-10-29 conversation choices. All words here are synthetic.
use super::*;

fn local_reader(text: &'static [u8]) -> Engine {
    let (s, _) = talk_state(BODY, None);
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        talk::choices::start(&mut st.borrow_mut());
        io.draw(|fb| {
            for y in 8..104 {
                for x in 176..312 {
                    fb.set(x, y, PAPER);
                }
            }
        });
        crate::ui::printf::printf(&st, &io, text, &[]).await;
        loop {
            let (_, word) = crate::ui::keyread::read_said(&st, &io, 50).await;
            st.borrow_mut().converse.input = word;
        }
    });
    for _ in 0..30 {
        e.tick(&[], 16);
    }
    e
}

fn visual_ids(e: &Engine) -> Vec<usize> {
    let mut b = e.state().ui.talk.buttons.clone();
    b.sort_by_key(|b| (b.row, b.col));
    b.iter().map(|b| b.id).collect()
}

#[test]
fn initial_choices_have_three_columns_and_no_paging() {
    let e = local_reader(b"Hello.\n");
    assert_eq!(visual_ids(&e), [2, 1, 0]);
    let st = e.state();
    assert!(st.ui.choices.arrows.is_empty());
    assert_eq!(st.ui.choices.topics.len(), 3);
    assert!(
        st.ui
            .talk
            .buttons
            .iter()
            .all(|b| b.row == 9 && b.cells.len() == 5)
    );
}

#[test]
fn used_grey_is_selected_from_the_glyph_renderers_sixteen_colours() {
    let (s, _) = talk_state(BODY, None);
    let mut e = Engine::with_game(s, Files::default(), |_, io| async move {
        io.draw(|fb| {
            fb.palette.fill(u6_render::Rgb::new(0, 0, 0));
            fb.palette[1] = u6_render::Rgb::new(100, 100, 100);
            // A closer grey in the memory palette must not wrap to a game colour via & 0x0f.
            fb.palette[32] = u6_render::Rgb::new(128, 128, 128);
        });
        assert_eq!(talk::choices::grey(&io, PAPER), 1);
    });
    finish(&mut e);
}

#[test]
fn yes_no_overlay_preserves_the_unflushed_question_after_answering() {
    let prompt = b"\n\n\n\n\n\n\n\nChoose the synthetic answer.";
    let mut frames = Vec::new();
    for reader in [b"\xf8yn".as_slice(), b"\n\xf8ynx".as_slice()] {
        let body = [prompt.as_slice(), reader, b"\xefy\xf6Done.\xb6"].concat();
        let (s, f) = talk_state(&body, None);
        let mut e = Engine::with_game(s, f, |st, io| async move {
            begin(&super::super::Cx::new(&st, &io), 0, 3).await;
            io.present();
        });
        for _ in 0..12 {
            e.tick(&[], 16);
        }
        e.tick(&[InputEvent::Key(Key::Char('y'))], 16);
        finish(&mut e);
        frames.push(e.frame().pixels().to_vec());
    }
    assert_eq!(
        frames[0], frames[1],
        "temporary answers restore the same flushed question as an equivalent key reader with room for an answer"
    );
}

#[test]
fn the_answer_row_does_not_introduce_an_extra_more_prompt() {
    let (s, _) = talk_state(BODY, None);
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        crate::ui::printf::printf(&st, &io, b"\n\n\n\n\n\n\n\n\nQuestion?", &[]).await;
        {
            let mut s = st.borrow_mut();
            s.ui.choices.yes_no = true;
            s.ui.text.more_rows = 1;
            s.ui.text.win_mut().flags |= crate::ui::text::PAGED;
        }
        let key = crate::ui::target::wait_event(&st, &io).await;
        assert_eq!(key, u16::from(b'Y'));
        talk::choices::end_answer(&mut st.borrow_mut(), &io);
    });
    for _ in 0..12 {
        e.tick(&[], 16);
    }
    assert!(
        talk::choices::answering(&e.state()),
        "answer choices are available without an extra paging key"
    );
    let b = e.state().ui.choices.answers[0].clone();
    click(
        &mut e,
        (i16::from(b.col) * 8 + 4, i16::from(b.row) * 8 + 4),
        POINTER_LEFT,
    );
    finish(&mut e);
}

#[test]
fn discovered_keywords_deduplicate_persist_page_and_keep_exit_accessible() {
    let mut e = local_reader(b"@one @two @three @four @five @six @seven @eight @nine @one.\n");
    assert_eq!(
        e.state().ui.choices.topics.len(),
        12,
        "nine unique discoveries"
    );
    assert_eq!(e.state().ui.talk.buttons.len(), 9);
    assert_eq!(e.state().ui.choices.arrows.len(), 2);
    let before = visual_ids(&e);
    let arrow = e.state().ui.choices.arrows[1].clone();
    click(
        &mut e,
        (i16::from(arrow.col) * 8 + 4, i16::from(arrow.row) * 8 + 4),
        POINTER_LEFT,
    );
    let after = visual_ids(&e);
    assert_eq!(after[0], talk::choices::EXIT);
    assert_eq!(after.len(), 4);
    assert!(after[1..].iter().all(|id| !before.contains(id)));
    assert!(
        e.state().converse.input.is_empty(),
        "paging submits no word"
    );
    let arrow = e.state().ui.choices.arrows[0].clone();
    click(
        &mut e,
        (i16::from(arrow.col) * 8 + 4, i16::from(arrow.row) * 8 + 4),
        POINTER_LEFT,
    );
    assert_eq!(visual_ids(&e), before);
}

#[test]
fn selected_words_move_after_unused_and_remain_selectable() {
    let mut e = local_reader(b"@cat @dog.\n");
    assert_eq!(visual_ids(&e), [2, 1, 0, 3, 4]);
    let xy = button_pixel(&e, 1);
    click(&mut e, xy, POINTER_LEFT);
    idle(&mut e, 4);
    assert_eq!(e.state().converse.input, b"name");
    assert_eq!(visual_ids(&e), [2, 0, 3, 4, 1]);
    assert!(
        e.state()
            .ui
            .talk
            .buttons
            .iter()
            .find(|b| b.id == 1)
            .unwrap()
            .used
    );
    let xy = button_pixel(&e, 1);
    click(&mut e, xy, POINTER_LEFT);
    idle(&mut e, 4);
    assert_eq!(e.state().converse.input, b"name", "used remains clickable");
    assert_eq!(
        e.state().ui.choices.topics.len(),
        5,
        "next read retains discoveries"
    );
}

#[test]
fn pressing_one_choice_and_releasing_another_never_submits() {
    let mut e = local_reader(b"@cat.\n");
    let (a, b) = (button_pixel(&e, 0), button_pixel(&e, 1));
    e.tick(&[at(a.0, a.1, POINTER_LEFT)], 16);
    e.tick(&[at(b.0, b.1, 0)], 16);
    assert!(e.state().converse.input.is_empty());
    assert!(e.state().ui.choices.topics.iter().all(|t| !t.used));
}

#[test]
fn a_press_keeps_its_identity_even_if_choices_are_reordered_before_release() {
    let (s, _) = talk_state(BODY, None);
    let handle = Rc::new(std::cell::RefCell::new(None));
    let saved = handle.clone();
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        saved.replace(Some((st.clone(), io.clone())));
        io.draw(|fb| {
            for y in 8..104 {
                for x in 176..312 {
                    fb.set(x, y, PAPER);
                }
            }
        });
        talk::begin(&mut st.borrow_mut(), &io);
        loop {
            talk::pointer(&mut st.borrow_mut(), &io, 0);
            io.present();
            io.vblank().await;
        }
    });
    e.tick(&[], 16);
    let old = button_pixel(&e, 1);
    talk::INJECTED.with(|k| k.borrow_mut().clear());
    e.tick(&[at(old.0, old.1, POINTER_LEFT)], 16);
    {
        let handle = handle.borrow();
        let (st, io) = handle.as_ref().unwrap();
        let mut s = st.borrow_mut();
        talk::choices::used(&mut s, b"name");
        talk::choices::layout(&mut s, io);
        assert_eq!(s.ui.talk.pressed, Some(Target::Button(1)));
        assert_eq!(talk::target_at(&s, old.0, old.1), Some(Target::Button(0)));
    }
    e.tick(&[at(old.0, old.1, 0)], 16);
    assert!(
        talk::INJECTED.with(|k| k.borrow().is_empty()),
        "old position cannot submit the different word now there"
    );
    let new = button_pixel(&e, 1);
    click(&mut e, new, POINTER_LEFT);
    assert_eq!(talk::INJECTED.with(|k| k.borrow().clone()), typed("name\n"));
}

#[test]
fn long_labels_have_ellipsis_full_hover_preview_and_full_input() {
    let mut e = local_reader(b"@alphabet @alphanumeric.\n");
    assert_eq!(
        e.state()
            .ui
            .talk
            .buttons
            .iter()
            .find(|b| b.id == 3)
            .unwrap()
            .cells,
        b"al...".iter().map(|&b| u16::from(b)).collect::<Vec<_>>()
    );
    let header = |e: &Engine| {
        let fb = e.frame();
        (16..72)
            .flat_map(|y| (176..312).map(move |x| (x, y)))
            .map(|(x, y)| fb.get(x, y))
            .collect::<Vec<_>>()
    };
    let original_header = header(&e);
    let a = button_pixel(&e, 3);
    let b = button_pixel(&e, 4);
    e.tick(&[at(a.0, a.1, 0)], 16);
    let first = header(&e);
    e.tick(&[at(b.0, b.1, 0)], 16);
    assert_ne!(
        header(&e),
        first,
        "same abbreviated label has a different full preview"
    );
    e.tick(&[at(0, 0, 0)], 16);
    assert_eq!(
        header(&e),
        original_header,
        "leaving the preview restores the entire header"
    );
    click(&mut e, b, POINTER_LEFT);
    idle(&mut e, 3);
    assert_eq!(e.state().converse.input, b"alphanumeric");
}

#[test]
fn new_conversation_resets_choices_and_does_not_relearn_old_visible_spans() {
    let (s, f) = talk_state(KEYED, None);
    let mut e = Engine::with_game(s, f, |st, io| async move {
        let cx = super::super::Cx::new(&st, &io);
        begin(&cx, 0, 3).await;
        assert!(
            st.borrow().ui.choices.topics.is_empty(),
            "end drops session state"
        );
        talk::choices::start(&mut st.borrow_mut());
        talk::choices::discover(&mut st.borrow_mut().ui, false);
        assert_eq!(
            st.borrow().ui.choices.topics.len(),
            3,
            "old inline spans not discoveries"
        );
    });
    for _ in 0..15 {
        e.tick(&[], 16);
    }
    let xy = button_pixel(&e, 3);
    click(&mut e, xy, POINTER_LEFT);
    finish(&mut e);
    assert!(e.state().ui.choices.topics.iter().all(|t| !t.used));
}

#[test]
fn wrapped_keyword_keeps_its_full_identity_after_prefix_scrolls_out() {
    let (mut s, _) = talk_state(BODY, None);
    talk::choices::start(&mut s);
    // A keyword longer than the message viewport. Only synthetic cells, real wrap/scroll hooks.
    for (r, word) in [(0, b"alpha".as_slice()), (1, b"beta".as_slice())] {
        for (c, &ch) in word.iter().enumerate() {
            s.ui.text.keys.draw(r, c as u8, u16::from(ch), 3, true);
            talk::choices::note(&mut s.ui, r, c as u8);
        }
        s.ui.text.keys.wrapped(r, (word.len() - 1) as u8, vec![32]);
    }
    talk::scroll(&mut s.ui);
    for (c, &ch) in b"gamma".iter().enumerate() {
        s.ui.text.keys.draw(1, c as u8, u16::from(ch), 3, true);
        talk::choices::note(&mut s.ui, 1, c as u8);
    }
    talk::choices::discover(&mut s.ui, false);
    assert_eq!(s.ui.choices.topics.len(), 4);
    assert_eq!(s.ui.choices.topics[3].word, b"alpha beta gamma");
}

#[test]
fn yes_no_buttons_only_follow_the_actual_restricted_reader() {
    for (body, expected) in [
        (
            b"Choose.\xf8yn\xefy\xf6Yes.\xb6\xefn\xf6No.\xb6".as_slice(),
            true,
        ),
        (b"Choose.\xf8abc\xefa\xf6A.\xb6".as_slice(), false),
        (b"Choose.\xf8ynx\xefy\xf6A.\xb6".as_slice(), false),
        (b"(y/n)\xfa\xefy\xf6A.\xb6".as_slice(), false),
        (b"(y/n)\xf9\x05\xb3$5\xb6".as_slice(), false),
        (b"(y/n)\xfb\x04\xb2#4\xb6".as_slice(), false),
        (b"(y/n)\xfc\x04\xb2#4\xb6".as_slice(), false),
    ] {
        let (s, f) = talk_state(body, None);
        let e = waiting(s, f);
        assert_eq!(talk::choices::answering(&e.state()), expected);
        assert_eq!(
            e.state().ui.choices.answers.len(),
            if expected { 2 } else { 0 }
        );
    }
}

#[test]
fn yes_no_clicks_send_only_the_matching_key_and_equal_typing() {
    let body = b"Choose.\xf8yn\xefy\xf6Yes.\xb6\xefn\xf6No.\xb6";
    for (id, key) in [(0, 'y'), (1, 'n')] {
        let (s, f) = talk_state(body, None);
        let mut clicked = waiting(s.clone(), f.clone());
        let b = clicked.state().ui.choices.answers[id].clone();
        talk::INJECTED.with(|k| k.borrow_mut().clear());
        click(
            &mut clicked,
            (i16::from(b.col) * 8 + 4, i16::from(b.row) * 8 + 4),
            POINTER_LEFT,
        );
        for _ in 0..10 {
            clicked.tick(&[], 16);
        }
        finish(&mut clicked);
        assert_eq!(
            talk::INJECTED.with(|k| k.borrow().clone()),
            [InputEvent::Key(Key::Char(key))]
        );
        let mut typed = waiting(s, f);
        typed.tick(&[InputEvent::Key(Key::Char(key))], 16);
        finish(&mut typed);
        assert_eq!(clicked.state().converse.input, typed.state().converse.input);
        assert_eq!(clicked.frame().pixels(), typed.frame().pixels());
        assert!(!talk::choices::answering(&clicked.state()));
        assert!(clicked.state().ui.choices.answers.is_empty());
    }
}

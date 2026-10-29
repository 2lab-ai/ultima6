//! Private, content-free receipts with the owner's archive and optional Korean pack.
#![allow(clippy::disallowed_methods)]
use super::*;
use crate::engine::POINTER_LEFT;
use crate::ui::talk::choices;
use std::rc::Rc;

fn localised_base(files: &Files) -> (State, bool) {
    let mut base = with_text(files);
    let korean = if let Some(path) = std::env::var_os("U6_KO_PACK") {
        let bytes = std::fs::read(path).quiet();
        let pack = crate::locale::Pack::parse(&bytes).quiet();
        let table = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../site/fonts/galmuri7-hangul.bin"
        ));
        let hangul = u6_formats::font::HangulFont::parse(table).quiet();
        crate::locale::install(&mut base, files, Rc::new(pack), Some(Rc::new(hangul))).quiet();
        true
    } else {
        false
    };
    (base, korean)
}

#[test]
fn real_answer_ui_preserves_conversation_content_and_order() {
    let Some(files) = files() else {
        assert_ne!(
            std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
            Ok("1"),
            "required archive unavailable"
        );
        return;
    };
    let (base, _) = localised_base(&files);
    let mut compared = 0;
    for npc in near_the_start(&base).into_iter().take(40) {
        let mut s = base.clone();
        if name_from_script(&mut s, &files, npc).0.is_empty() {
            continue;
        }
        let lines = ["name", "job", "bye"];
        let run = |enabled| {
            choices::ANSWER_UI.with(|v| v.set(enabled));
            crate::ui::print::TRANSCRIPT_ALL.with(|all| all.replace(Some(Vec::new())));
            let e = converse_watched(&base, &files, npc, &lines, false, |_| {});
            let mut transcript = crate::ui::print::TRANSCRIPT_ALL.with(|all| all.take().unwrap());
            transcript.push(lines_of(&e.state()).pop().unwrap_or_default());
            let content: String = transcript
                .concat()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            (e, content)
        };
        let (before, before_text) = run(false);
        let (after, after_text) = run(true);
        // Collect before the normal 64-line buffer truncates: extra answer line breaks must
        // not make an otherwise identical conversation look shorter at its end.
        assert!(
            before_text == after_text,
            "answer UI changed full conversation content/order, actor slot {npc}"
        );
        assert!(
            before.state().converse == after.state().converse,
            "answer UI changed VM state, actor slot {npc}"
        );
        assert_eq!(before.finished(), after.finished());
        compared += 1;
    }
    assert!(compared >= 3);
    println!(
        "answer UI paired archive receipt: {compared} conversations preserve non-whitespace content/order and VM state"
    );
}

fn picture(e: &mut Engine, label: &str, korean: bool) {
    if label != "preview" {
        e.tick(
            &[InputEvent::Pointer {
                x: 0,
                y: 0,
                buttons: 0,
            }],
            16,
        );
        e.tick(&[], 16);
    }
    let Some(dir) = std::env::var_os("U6_CHOICES_FRAMES") else {
        return;
    };
    let dir = std::fs::canonicalize(dir).expect("an existing private frames directory");
    let repo = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).quiet();
    assert!(
        !dir.starts_with(repo),
        "game frames stay outside the repository"
    );
    let (w, h) = (u6_render::WIDTH, u6_render::HEIGHT);
    let mut rgba = Vec::new();
    e.frame().write_rgba(&mut rgba);
    let lang = if korean { "ko" } else { "en" };
    std::fs::write(
        dir.join(format!("choices-{lang}-{label}-{w}x{h}.rgba")),
        rgba,
    )
    .quiet();
}

fn click(e: &mut Engine, row: u8, col: u8) {
    let (x, y) = (i16::from(col) * 8 + 4, i16::from(row) * 8 + 4);
    e.tick(
        &[
            InputEvent::Pointer {
                x,
                y,
                buttons: POINTER_LEFT,
            },
            InputEvent::Pointer { x, y, buttons: 0 },
        ],
        16,
    );
}

#[test]
fn real_inventory_panel_has_exit_and_is_restored_after_the_word_reader() {
    let Some(files) = files() else {
        assert_ne!(
            std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
            Ok("1"),
            "required archive unavailable"
        );
        return;
    };
    let s = with_text(&files);
    let mut e = Engine::with_game(s, files, |st, io| async move {
        io.enable_src_plane();
        {
            let mut s = st.borrow_mut();
            s.converse.active = true;
            choices::start(&mut s);
            io.draw(|fb| {
                for (dst, c) in fb.palette.iter_mut().zip(&s.tables.palette) {
                    *dst = u6_render::Rgb::new(c.r, c.g, c.b);
                }
            });
            crate::main_loop::draw_paper(&mut s, &io);
        }
        // The exact op be panel entry point, with the original inventory art and glyphs.
        crate::converse::seams::inventory_panel(&Cx::new(&st, &io), 1, Some(0), 0).await;
        let capture = || {
            io.draw(|fb| {
                (6..104)
                    .flat_map(|y| (168..312).map(move |x| (x, y)))
                    .map(|(x, y)| (fb.get(x, y), fb.src_at(x, y)))
                    .collect::<Vec<_>>()
            })
        };
        let before = capture();
        let paper = st.borrow().ui.tables.colours.paper;
        assert!(
            io.draw(|fb| (96..104).any(|y| (176..312).any(|x| fb.get(x, y) != Some(paper)))),
            "the old blank-row guard would hide choices"
        );
        let (_, word) = crate::ui::keyread::read_said(&st, &io, 50).await;
        assert!(word == b"bye", "exit submits the real ending keyword");
        choices::finish(&mut st.borrow_mut(), &io);
        assert!(
            capture() == before,
            "original inventory pixels and source metadata restored"
        );
        io.present();
    });
    for _ in 0..20 {
        e.tick(&[], 16);
    }
    let b = e
        .state()
        .ui
        .talk
        .buttons
        .iter()
        .find(|b| b.id == choices::EXIT)
        .cloned()
        .quiet();
    picture(&mut e, "inventory-choices", false);
    click(&mut e, b.row, b.col);
    for _ in 0..20 {
        e.tick(&[], 16);
    }
    assert!(e.finished());
    assert!(e.state().ui.choices.topics.is_empty());
    picture(&mut e, "inventory-restored", false);
    println!("original inventory choice/exit/restoration receipt: passed");
}

#[test]
fn real_choices_click_through_used_overflow_reset_and_yes_no() {
    let files = files();
    if files.is_none() {
        assert_ne!(
            std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
            Ok("1"),
            "required archive unavailable"
        );
        return;
    }
    let files = files.quiet();
    let (base, korean) = localised_base(&files);
    let mut counts = [0usize; 5]; // grids, used, overflow, y/n, clean exits
    let mut reset_picture = false;
    let mut preview_picture = false;
    for npc in near_the_start(&base).into_iter().take(40) {
        let s = base.clone();
        let leader = s.actors.party.leader_index;
        let mut e = Engine::with_game(s, files.clone(), move |st, io| async move {
            {
                let mut s = st.borrow_mut();
                io.draw(|fb| {
                    for (dst, c) in fb.palette.iter_mut().zip(&s.tables.palette) {
                        *dst = u6_render::Rgb::new(c.r, c.g, c.b);
                    }
                });
                crate::main_loop::draw_paper(&mut s, &io);
            }
            talk(&Cx::new(&st, &io), leader, npc, true).await;
        });
        let mut first_grid = true;
        for pass in 0..300 {
            for _ in 0..4 {
                e.tick(&[], 16);
            }
            if e.finished() {
                assert!(
                    e.state().ui.choices.topics.is_empty(),
                    "conversation exit resets choices"
                );
                counts[4] += 1;
                break;
            }
            let st = e.state().clone();
            if choices::answering(&st) {
                if counts[3] == 0 {
                    picture(&mut e, "yes-no", korean);
                }
                counts[3] += 1;
                let b = &st.ui.choices.answers[1];
                click(&mut e, b.row, b.col);
            } else if !st.ui.talk.buttons.is_empty() {
                if counts[0] == 0 {
                    picture(&mut e, "initial", korean);
                }
                if first_grid && !reset_picture && counts[1] > 0 {
                    assert!(
                        st.ui.choices.topics.iter().all(|t| !t.used),
                        "new conversation has no used choices"
                    );
                    picture(&mut e, "reset", korean);
                    reset_picture = true;
                }
                first_grid = false;
                counts[0] += 1;
                if st.ui.talk.buttons.iter().any(|b| b.used) {
                    if counts[1] == 0 {
                        picture(&mut e, "used", korean);
                    }
                    counts[1] += 1;
                }
                if !st.ui.choices.arrows.is_empty() {
                    if counts[2] == 0 {
                        picture(&mut e, "overflow", korean);
                        let b = &st.ui.choices.arrows[1];
                        click(&mut e, b.row, b.col);
                        e.tick(&[], 16);
                        picture(&mut e, "page-two", korean);
                        assert!(
                            e.state()
                                .ui
                                .talk
                                .buttons
                                .iter()
                                .any(|b| b.id == choices::EXIT)
                        );
                        let b = e.state().ui.choices.arrows[0].clone();
                        click(&mut e, b.row, b.col);
                    }
                    if !preview_picture
                        && let Some(b) = st
                            .ui
                            .talk
                            .buttons
                            .iter()
                            .find(|b| st.ui.choices.topics[b.id].label.len() > b.cells.len())
                            .cloned()
                    {
                        let header = |e: &Engine| {
                            let fb = e.published_frame();
                            (16..72)
                                .flat_map(|y| (176..312).map(move |x| (x, y)))
                                .map(|(x, y)| (fb.get(x, y), fb.src_at(x, y)))
                                .collect::<Vec<_>>()
                        };
                        let original = header(&e);
                        e.tick(
                            &[InputEvent::Pointer {
                                x: i16::from(b.col) * 8 + 4,
                                y: i16::from(b.row) * 8 + 4,
                                buttons: 0,
                            }],
                            16,
                        );
                        picture(&mut e, "preview", korean);
                        e.tick(
                            &[InputEvent::Pointer {
                                x: 0,
                                y: 0,
                                buttons: 0,
                            }],
                            16,
                        );
                        assert!(
                            header(&e) == original,
                            "leaving full-word preview restores the original portrait/name header"
                        );
                        preview_picture = true;
                    }
                    counts[2] += 1;
                }
                let b = st
                    .ui
                    .talk
                    .buttons
                    .iter()
                    .filter(|b| b.id != choices::EXIT && !b.used)
                    .min_by_key(|b| b.id)
                    .or_else(|| st.ui.talk.buttons.iter().find(|b| b.id == choices::EXIT))
                    .quiet();
                click(&mut e, b.row, b.col);
            } else {
                let key = if st.ui.input.line_input {
                    Key::Enter
                } else {
                    Key::Char([' ', 'n', 'y'][pass % 3])
                };
                e.tick(&[InputEvent::Key(key)], 16);
            }
        }
    }
    assert!(
        reset_picture,
        "new conversation reset exercised after a used choice"
    );
    println!(
        "choice receipts: grids {} used {} overflow {} yn {} exits {}",
        counts[0], counts[1], counts[2], counts[3], counts[4]
    );
    assert!(
        counts[0] > 0 && counts[1] > 0 && counts[2] > 0 && counts[3] > 0 && counts[4] > 0,
        "every required path exercised"
    );
}

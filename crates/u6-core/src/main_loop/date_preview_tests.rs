//! Local original-data receipts; only boolean checks/counts leave the runtime fixture.
use crate::{Engine, Files, State};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use u6_render::Framebuffer;
use u6_testkit::{Quiet, assert_same};

#[allow(clippy::disallowed_methods)] // Read-only owner archive for this real-data receipt.
fn files() -> Option<Files> {
    let archive = u6_testkit::archive();
    if !archive.is_file() {
        return None;
    }
    let data = std::fs::read(archive).quiet();
    let loaded = u6_intake::intake(vec![u6_intake::InputFile {
        name: "owned.zip".into(),
        data,
    }]);
    Some(Files::new(loaded.files))
}

fn fingerprint(frame: &Framebuffer, map: bool) -> (u64, u64) {
    let (x0, y0, x1, y1) = crate::geom::SHOWN;
    let mut pixels = std::collections::hash_map::DefaultHasher::new();
    let mut sources = std::collections::hash_map::DefaultHasher::new();
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let inside = (usize::from(x0)..=usize::from(x1)).contains(&x)
                && (usize::from(y0)..=usize::from(y1)).contains(&y);
            if inside == map {
                let i = y * frame.width() + x;
                frame.pixels()[i].hash(&mut pixels);
                frame.src().quiet()[i].hash(&mut sources);
            }
        }
    }
    (pixels.finish(), sources.finish())
}

#[test]
fn date_line_publication_changes_ui_but_neither_map_pixels_nor_sources() {
    date_line_receipt(|_, _| {});
}

fn date_line_receipt(configure: impl FnOnce(&mut State, &Files)) {
    let Some(files) = files() else { return };
    let mut s = State::new_game(&files).quiet();
    crate::main_loop::load_state_tail(&mut s);
    s.ui.text.main_windows(&s.ui.tables);
    configure(&mut s, &files);
    let before_proof = s.clone();
    let before_counts = [s.locale.texts.get(), s.locale.tables.get()];
    assert!(crate::actors::clock::date_preview_safe(&s, &files));
    assert!(
        s == before_proof,
        "date proof changes no live state or locale counts"
    );
    assert_eq!([s.locale.texts.get(), s.locale.tables.get()], before_counts);
    crate::map::view::update_view(&mut s);
    let queued = Rc::new(RefCell::new(None));
    let q = Rc::clone(&queued);
    let mut e = Engine::with_game(s, files, move |st, io| async move {
        crate::gfx::render_window(&mut st.borrow_mut(), &io, true);
        io.present();
        io.vblank().await;
        let rng = st.borrow().rng;
        let date = st.borrow().actors.calendar;
        crate::actors::clock::date_line(&mut st.borrow_mut());
        *q.borrow_mut() = Some(st.borrow().main.pending_print.clone());
        super::flush_prints(&st, &io).await;
        assert_same!([st.borrow().rng], [rng], "date UI preserves RNG");
        assert_same!(
            [st.borrow().actors.calendar],
            [date],
            "date UI preserves calendar"
        );
        io.present();
        io.vblank().await;
    });
    e.enable_src_plane();
    e.set_motion(true);
    e.tick(&[], 20);
    let before = e.published_frame().clone();
    let first = e.take_motion();
    assert!(first.iter().any(|p| p.shot.is_some()));
    for _ in 0..20 {
        e.tick(&[], 20);
        if e.finished() {
            break;
        }
    }
    assert!(e.finished());
    let after = e.published_frame();
    let map_before = fingerprint(&before, true);
    let map_after = fingerprint(&after, true);
    let ui_before = fingerprint(&before, false);
    let ui_after = fingerprint(&after, false);
    assert!(
        map_before == map_after,
        "date output preserves map pixels/Src"
    );
    assert!(ui_before != ui_after, "date output visibly updates UI");
    assert_eq!(
        queued.borrow().as_ref().quiet().len(),
        4,
        "original queued operations"
    );
    eprintln!("date receipt: unchanged map+Src; changed UI; queued_ops=4");
    drop(after);
    assert!(
        e.take_motion().is_empty(),
        "date-only publication is not foreign"
    );
}

#[test]
fn date_line_publication_with_long_or_multiline_translations() {
    for (date, multiline) in [(true, false), (true, true), (false, false), (false, true)] {
        date_line_receipt(|s, files| {
            let io = crate::Io::new(files.clone());
            s.actors.wind = 0xff;
            let english = if date {
                super::game_string(&io, crate::string_registry::text!(-0x7293))
            } else {
                super::pointed_string(
                    &io,
                    crate::string_registry::text_table!(0x7ae8).get(0).quiet(),
                )
            };
            let mut translated = if multiline {
                "Z\n".repeat(80)
            } else {
                "Z".repeat(400)
            };
            if date {
                translated.extend(english.iter().map(|&b| char::from(b)));
            }
            let p = crate::locale::Pack::new(
                crate::ui::Language::Korean,
                "test/1",
                crate::locale::fingerprint([]),
                [],
            )
            .quiet()
            .with_strings([(
                crate::converse::disasm::catalog::hash64(&english),
                crate::locale::StringText {
                    len: english.len() as u32,
                    text: translated.into_bytes(),
                },
            )])
            .quiet();
            crate::locale::install(s, files, Rc::new(p), None).quiet();
        });
    }
}

#[test]
#[allow(clippy::disallowed_methods)] // Read-only local locale pack; never emitted.
fn date_line_publication_with_the_owned_korean_pack() {
    let path = u6_testkit::data_path_with("U6_LOCALE", "i18n/ko-1026a/ko.u6lp");
    if !path.is_file() {
        assert!(
            std::env::var_os("U6_LOCALE").is_none(),
            "explicit U6_LOCALE input is missing"
        );
        eprintln!("skipped: optional local Korean locale pack; set U6_LOCALE to require it");
        return;
    }
    let pack = crate::locale::Pack::parse(&std::fs::read(path).quiet()).quiet();
    let hangul = u6_formats::font::HangulFont::parse(include_bytes!(
        "../../../../site/fonts/galmuri7-hangul.bin"
    ))
    .quiet();
    date_line_receipt(|s, files| {
        crate::locale::install(s, files, Rc::new(pack), Some(Rc::new(hangul))).quiet();
    });
}

#[test]
fn prediction_omits_only_date_queue_from_the_real_minute() {
    let Some(files) = files() else { return };
    let mut s = State::empty();
    s.ui.text.main_windows(&s.ui.tables);
    s.actors.calendar.hour = 12;
    s.actors.calendar.minute = 30;
    s.actors.work.last_hour = 12;
    s.actors.set_tail(crate::actors::tail::LIGHT, 7);
    let mut e = Engine::with_game(s, files, |st, _io| async move {
        let mut predicted = st.borrow().clone();
        let before_rng = predicted.rng;
        let permitted = crate::engine::predict_without_io(|| {
            crate::actors::clock::tick(&mut predicted, 1);
        });
        assert!(
            permitted.is_some(),
            "the date-only minute needs no live I/O"
        );
        assert!(
            predicted.main.pending_print.is_empty(),
            "only date output is omitted"
        );
        crate::actors::clock::tick(&mut st.borrow_mut(), 1);
        let real = st.borrow();
        assert!(
            matches!(
                real.main.pending_print.as_slice(),
                [
                    super::Pending::Window(crate::ui::WindowId::Screen),
                    super::Pending::Cursor(2, 21),
                    super::Pending::Print(_, _),
                    super::Pending::Window(crate::ui::WindowId::Messages),
                ]
            ),
            "runtime retains every original date operation in order"
        );
        assert!(
            predicted.rng != before_rng,
            "the predictor ran the minute's actual RNG draws"
        );
        predicted.main.pending_print = real.main.pending_print.clone();
        assert!(
            predicted == *real,
            "every other State field and RNG matches the actual minute"
        );
    });
    for _ in 0..20 {
        e.tick(&[], 20);
        if e.finished() {
            break;
        }
    }
    assert!(e.finished());
}

#[test]
fn date_preview_rejects_unsafe_geometry_scroll_paging_and_interactive_translation() {
    let Some(files) = files() else { return };
    let mut s = State::new_game(&files).quiet();
    s.ui.text.main_windows(&s.ui.tables);
    let screen = crate::ui::WindowId::Screen.index();
    for flag in [crate::ui::text::SCROLL, crate::ui::text::PAGED] {
        let mut altered = s.clone();
        altered.ui.text.windows[screen].flags |= flag;
        assert!(!crate::actors::clock::date_preview_safe(&altered, &files));
    }
    let mut altered = s.clone();
    altered.ui.text.windows[screen].height = 20;
    assert!(!crate::actors::clock::date_preview_safe(&altered, &files));
    let mut altered = s.clone();
    altered.ui.text.current = crate::ui::WindowId::Screen;
    assert!(!crate::actors::clock::date_preview_safe(&altered, &files));

    let io = crate::Io::new(files.clone());
    let english = super::game_string(&io, crate::string_registry::text!(-0x7293));
    let mut translated = b"*".to_vec();
    translated.extend_from_slice(&english);
    let pack = crate::locale::Pack::new(
        crate::ui::Language::Korean,
        "test/1",
        crate::locale::fingerprint([]),
        [],
    )
    .quiet()
    .with_strings([(
        crate::converse::disasm::catalog::hash64(&english),
        crate::locale::StringText {
            len: english.len() as u32,
            text: translated,
        },
    )])
    .quiet();
    crate::locale::install(&mut s, &files, Rc::new(pack), None).quiet();
    assert!(!crate::actors::clock::date_preview_safe(&s, &files));
    let mut e = Engine::with_game(s, files, |st, io| async move {
        crate::actors::clock::date_line(&mut st.borrow_mut());
        super::flush_prints(&st, &io).await;
    });
    e.tick(&[], 20);
    assert!(
        !e.finished(),
        "the translated star really waits for an input"
    );
    for _ in 0..40 {
        e.tick(&[crate::InputEvent::Key(crate::Key::Space)], 20);
        if e.finished() {
            break;
        }
    }
    assert!(e.finished(), "fresh input completes the date wait");
}

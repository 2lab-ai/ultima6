//! E1: runtime fixtures are built from the owner's disk state, not a completed user save.
//! Reads and optional receipts stay outside Git; assertions never print original contents.
#![allow(clippy::disallowed_methods)]
use super::*;
use crate::main_loop::Program;
use crate::objects::{self, Slot};
use crate::{Engine, InputEvent, Key};
use std::rc::Rc;
use u6_testkit::{Quiet, assert_same};
mod launcher;
fn save_frame(e: &Engine, name: &str) {
    let Some(dir) = std::env::var_os("U6_END_FRAMES") else {
        return;
    };
    let dir = std::fs::canonicalize(dir).quiet();
    assert!(
        dir.ancestors().all(|p| !p.join(".git").exists()),
        "receipts must stay outside every Git checkout"
    );
    crate::usecmd::tests_real::save_frame(e, "U6_END_FRAMES", name);
}
fn files() -> Option<Files> {
    let archive = u6_testkit::archive();
    if !archive.exists() {
        eprintln!("ending: skipped, original archive absent");
        return None;
    }
    let result = u6_intake::intake(vec![u6_intake::InputFile {
        name: "owned.zip".into(),
        data: std::fs::read(archive).quiet(),
    }]);
    Some(Files::new(result.files))
}
fn fixture(files: &Files) -> (State, Slot, Slot) {
    let mut s = State::new_game(files).quiet();
    crate::main_loop::load_state_tail(&mut s);
    s.ui.text.main_windows(&s.ui.tables);
    s.ui.text.more_rows = 100;
    s.actors.objlist_tail[0x81] = 1;
    s.actors.objlist_tail[0x78] = 0;
    s.actors.objlist_tail[0x79] = 0xff;
    crate::map::travel::teleport(&mut s, 0x39b, 0x354, 0, 0);
    let codex = objects::object_of_kind_at(&mut s, 0x39b, 0x353, 0, 0x3b);
    assert!(
        codex >= 0,
        "the original disk state supplies the shrine object"
    );
    for (x, kind) in [(0x399, 0x18a), (0x39d, 0x18c)] {
        if objects::object_of_kind_at(&mut s, x, 0x353, 0, kind) < 0 {
            assert!(objects::create_on_map(&mut s.objects, kind, 0, 0, x, 0x353, 0) >= 0);
        }
    }
    let lead = s.actors.party.members[usize::from(s.actors.party.leader_index)];
    let cube = objects::create_in(&mut s, 0x3e, objects::IN_INVENTORY, 0, Slot::from(lead));
    assert!(cube >= 0);
    for f in 0..8 {
        assert!(objects::create_in(&mut s, 0x56 | f << 10, objects::IN_CONTAINER, 0, cube) >= 0);
    }
    s.ui.input.selection = cube;
    s.ui.input.target_pending = 1;
    s.ui.input.target = (0x39b, 0x354);
    s.ui.panel.member = i16::from(s.actors.party.leader_index);
    s.main.program = Program::Game;
    (s, cube, codex)
}
#[test]
fn elapsed_arguments_preserve_the_original_unsigned_day_comparison() {
    let c = crate::actors::Calendar {
        year: 163,
        month: 2,
        day: 1,
        ..Default::default()
    };
    assert_eq!(
        Elapsed::from_calendar(c),
        Elapsed {
            years: 1,
            months: 8,
            days: -3
        }
    );
    assert_eq!(
        Elapsed::from_calendar(crate::actors::Calendar {
            year: 161,
            month: 7,
            day: 4,
            ..c
        }),
        Elapsed::default()
    );
}
#[test]
fn original_resources_validate_and_duration_words_are_read_from_the_image() {
    let Some(files) = files() else { return };
    let a = Assets::load(&files).quiet();
    let mut expected = a.string(assets::A4 + 0xc4 + 13);
    expected.extend(a.string(0x103858));
    assert_same!(
        a.duration(1, 0x103858, false),
        expected,
        "singular elapsed word from runtime image"
    );
    let mut plural = a.string(assets::A4 + 0xc4 + 26);
    plural.extend(a.string(0x103848));
    plural.extend(a.string(0x10384e));
    assert_same!(
        a.duration(2, 0x103848, true),
        plural,
        "plural elapsed word from runtime image"
    );
    assert!(a.duration(0, 0x103848, true).is_empty());
    assert_eq!(a.dissolve.len(), 113 * 94);
    let mut unique = a.dissolve.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 113 * 94);
}
#[test]
fn launcher_elapsed_arguments_keep_distinct_year_month_and_day_units() {
    let Some(files) = files() else { return };
    let a = Assets::load(&files).quiet();
    let expected = [(1, 0x103848), (2, 0x103850), (3, 0x103858)].map(|(n, unit)| {
        let mut part = a.string(assets::A4 + 0xc4 + n * 13);
        part.extend(a.string(unit));
        if n != 1 {
            part.extend(a.string(0x10384e));
        }
        part
    });
    let sc = Scene::new(
        a,
        Elapsed {
            years: 1,
            months: 2,
            days: 3,
        },
    );
    assert_same!(sc.elapsed_parts(), expected, "launcher duration units");
}
#[test]
fn bad_resources_refuse_without_consuming_the_shrine_or_requesting_exit() {
    let Some(files) = files() else { return };
    for name in ["end", "end.new", "u6.set"] {
        for malformed in [Vec::new(), vec![0x55; 19]] {
            let (s, _, codex) = fixture(&files);
            let before = s.objects.tf(codex);
            let mut bad = files.clone();
            bad.overlay("broken", [(name.into(), malformed)].into());
            assert!(preflight(&bad).is_err());
            let mut e = Engine::with_game(s, bad, |st, io| async move {
                crate::usecmd::use_command(&st, &io).await;
            });
            for _ in 0..100 {
                e.tick(&[InputEvent::Key(Key::Space)], 20);
                if e.finished() {
                    break;
                }
            }
            assert!(e.finished());
            assert_same!(
                [e.state().objects.tf(codex)],
                [before],
                "invalid asset leaves shrine object"
            );
            assert!(e.state().main.ending.is_none());
            assert_eq!(e.state().main.program, Program::Game);
        }
    }
}
#[test]
fn original_state_fixture_uses_cube_and_launcher_state_two_to_completion() {
    let Some(files) = files() else { return };
    let (s, _, codex) = fixture(&files);
    let rng = s.rng;
    let date = s.actors.calendar;
    let expected = Elapsed::from_calendar(date);
    let mut e = Engine::with_game(s, files, |st, io| async move {
        crate::usecmd::use_command(&st, &io).await;
        assert!(
            st.borrow().main.ending.is_some(),
            "actual Use reaches the ending handoff"
        );
        crate::main_loop::finish_program(&st, &io, 2).await;
    });
    e.enable_src_plane();
    let mut seen_end = false;
    for tick in 0..8000 {
        let input = if tick % 47 == 0 {
            vec![InputEvent::Key(Key::Space)]
        } else {
            vec![]
        };
        e.tick(&input, 20);
        let is_end = e.state().main.program == Program::End;
        seen_end |= is_end;
        if is_end {
            assert!(
                e.sprite().is_none(),
                "end has its own empty hardware sprite"
            );
            assert!(
                e.frame()
                    .src()
                    .quiet()
                    .iter()
                    .all(|p| *p == crate::Src::CLASSIC),
                "ending frame has no game/map source pixels"
            );
        }
        if e.finished() {
            break;
        }
    }
    assert!(seen_end && e.finished(), "state 2 runs end and stops");
    assert_eq!(e.state().main.ending, Some(expected));
    assert_eq!(e.state().objects.kind(codex), 0);
    assert_eq!(
        e.state().actors.calendar,
        date,
        "ending never ticks the game calendar"
    );
    assert_eq!(e.state().rng, rng, "ending owns no game RNG draws");
    assert_eq!(e.state().main.program, Program::End);
}
#[test]
fn all_passages_animation_dissolves_music_final_key_and_restart_are_pure() {
    let Some(files) = files() else { return };
    let baseline = files.get("end").quiet().to_vec();
    let mut previous = None;
    for repetition in 0..2 {
        let a = Assets::load(&files).quiet();
        let expected_songs = a.songs.clone();
        let scene = Rc::new(RefCell::new(None));
        let out = Rc::clone(&scene);
        let mut e = Engine::with_game(
            State::new_game(&files).quiet(),
            files.clone(),
            move |st, io| async move {
                st.borrow_mut().main.program = Program::End;
                io.reset_src_plane();
                let mut sc = Scene::new(
                    a,
                    Elapsed {
                        years: 1,
                        months: 2,
                        days: 3,
                    },
                );
                sc.main(&st, &io).await;
                out.replace(Some(sc));
            },
        );
        let mut changed = 0;
        let mut last = Vec::new();
        for tick in 0..8000 {
            let input = if tick % 61 == 0 {
                vec![InputEvent::Key(Key::Space)]
            } else {
                vec![]
            };
            e.tick(&input, 20);
            let frame = e.frame();
            if frame.pixels() != last {
                changed += 1;
                last = frame.pixels().to_vec();
            }
            assert!(
                frame
                    .pixels()
                    .iter()
                    .enumerate()
                    .all(|(i, &c)| i % crate::WIDTH < 320 && i / crate::WIDTH < 200 || c == 0),
                "outside original scene cleared in both layouts"
            );
            if repetition == 0 && tick % 100 == 0 {
                save_frame(&e, &format!("frame-{tick:04}"));
            }
            if e.finished() {
                break;
            }
        }
        save_frame(&e, &format!("final-{repetition}"));
        assert!(e.finished(), "final key stops scene");
        assert!(
            changed > 100,
            "animation and intermediate dissolve frames visible"
        );
        let sc = scene.borrow_mut().take().quiet();
        let passages: std::collections::BTreeSet<_> = sc.receipts.iter().map(|x| x.0).collect();
        assert_eq!(
            passages,
            (0..16).collect(),
            "every original passage displayed"
        );
        let sounds = e
            .state()
            .sound
            .calls
            .iter()
            .map(|(_, x)| x.clone())
            .collect::<Vec<_>>();
        let loads = sounds
            .iter()
            .filter_map(|x| {
                if let MedCall::Load(n) = x {
                    Some(n.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_same!(loads, expected_songs, "music 10 then 11 then 12");
        assert!(
            matches!(sounds.last(), Some(MedCall::Stop)),
            "final music stop"
        );
        if let Some(ref frames) = previous {
            assert_same!(
                sc.receipts,
                frames,
                "fresh engine restart replays identical frames"
            );
        } else {
            previous = Some(sc.receipts);
        }
    }
    assert_same!(
        files.get("end").quiet(),
        baseline,
        "runtime input executable unchanged"
    );
}

#[test]
fn a_held_mouse_waits_for_release_and_preserves_the_key_queued_before_release() {
    let Some(files) = files() else { return };
    let a = Assets::load(&files).quiet();
    let taken = Rc::new(std::cell::Cell::new(0));
    let out = Rc::clone(&taken);
    let mut e = Engine::with_game(State::empty(), files, move |_, io| async move {
        let mut sc = Scene::new(a, Elapsed::default());
        sc.key(&io).await;
        out.set(1);
        sc.key(&io).await;
        out.set(2);
        sc.drain(&io);
        sc.key(&io).await;
        out.set(3);
    });
    e.tick(
        &[InputEvent::Pointer {
            x: 50,
            y: 50,
            buttons: 1,
        }],
        20,
    );
    for _ in 0..4 {
        e.tick(&[], 20);
    }
    assert_eq!(taken.get(), 0, "held mouse does not advance");
    e.tick(
        &[
            InputEvent::Key(Key::Space),
            InputEvent::Pointer {
                x: 50,
                y: 50,
                buttons: 0,
            },
        ],
        20,
    );
    for _ in 0..4 {
        e.tick(&[], 20);
    }
    assert_eq!(
        taken.get(),
        2,
        "release then the waiting key, without deadlock"
    );
    assert!(!e.finished(), "the final wait needs fresh input");
    e.tick(
        &[
            InputEvent::Pointer {
                x: 50,
                y: 50,
                buttons: 2,
            },
            InputEvent::Pointer {
                x: 50,
                y: 50,
                buttons: 0,
            },
        ],
        20,
    );
    for _ in 0..4 {
        e.tick(&[], 20);
    }
    assert_eq!(taken.get(), 3);
    assert!(e.finished());
}

#[test]
fn incomplete_shrine_keeps_playing() {
    let Some(files) = files() else { return };
    for kind in [0x18a, 0x18c, 0x56] {
        let (mut s, cube, codex) = fixture(&files);
        let slot = if kind == 0x56 {
            objects::first_child(&mut s.objects, cube)
        } else {
            let x = if kind == 0x18a { 0x399 } else { 0x39d };
            objects::object_of_kind_at(&mut s, x, 0x353, 0, kind)
        };
        assert!(slot >= 0);
        objects::remove(&mut s, slot);
        let mut e = Engine::with_game(s, files.clone(), |st, io| async move {
            crate::usecmd::use_command(&st, &io).await;
        });
        for _ in 0..100 {
            e.tick(&[InputEvent::Key(Key::Space)], 20);
            if e.finished() {
                break;
            }
        }
        assert!(e.finished());
        assert_eq!(e.state().objects.kind(codex), 0x3b);
        assert!(e.state().main.ending.is_none());
    }
}

/// Serialize a constructed end condition in the original save format. Callers either keep
/// the files in memory or explicitly write private host receipts; no completed user save.
fn runtime_save(files: &Files) -> std::collections::BTreeMap<String, Vec<u8>> {
    let (mut s, cube, _) = fixture(files);
    objects::move_to(&mut s, cube, 0x39b, 0x355, 0);
    s.actors.calendar.year = 162;
    s.actors.calendar.month = 9;
    s.actors.calendar.day = 7;
    let i = usize::from(s.actors.party.leader_index);
    s.actors.party.names[i] = "Tester".into();
    crate::saveload::save(&mut s);
    crate::saveload::changed_since(&s, 0)
}

/// Build a private end-condition save from the owner's disk state for host/UI receipts.
/// This is explicitly a constructed fixture, not an actual user's completed save.
#[test]
#[ignore = "writes a runtime save outside the repository"]
fn write_runtime_ending_fixture() {
    let Some(files) = files() else { return };
    let saved = runtime_save(&files);
    let root = std::path::PathBuf::from(
        std::env::var_os("U6_END_FIXTURE").expect("private output required"),
    );
    std::fs::create_dir_all(&root).quiet();
    let root = std::fs::canonicalize(root).quiet();
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .quiet()
        .parent()
        .quiet()
        .to_path_buf();
    assert!(!root.starts_with(repo), "save must stay outside checkout");
    assert!(
        root.ancestors().all(|p| !p.join(".git").exists()),
        "save must stay outside every Git checkout"
    );
    for (name, data) in &saved {
        let p = root.join(name);
        std::fs::create_dir_all(p.parent().quiet()).quiet();
        std::fs::write(p, data).quiet();
    }
    let mut reload = files.clone();
    reload.overlay("fixture", saved);
    let mut state = State::new_game(&reload).quiet();
    crate::main_loop::load_state_tail(&mut state);
    let found = objects::object_of_kind_at(&mut state, 0x39b, 0x355, 0, 0x3e);
    assert!(found >= 0, "saved cube is present after a fresh load");
    assert!(preflight(&reload).is_ok());
    eprintln!("ending fixture: saved and read back");
}

#[test]
fn dissolve_preserves_the_original_final_pixel_outside_the_rectangle() {
    let Some(files) = files() else { return };
    let a = Assets::load(&files).quiet();
    let out = Rc::new(RefCell::new(None));
    let result = Rc::clone(&out);
    let mut e = Engine::with_game(State::empty(), files, move |_, io| async move {
        let mut sc = Scene::new(a, Elapsed::default());
        mem::fill_view(&mut sc.chip, &View::screen(Ptr::at(Region::Screen0)), 4);
        mem::fill_view(&mut sc.chip, &View::screen(Ptr::at(Region::Work)), 3);
        sc.dissolve(&io).await;
        result.replace(Some(sc.chip));
    });
    for _ in 0..500 {
        e.tick(&[], 20);
        if e.finished() {
            break;
        }
    }
    assert!(e.finished());
    let chip = out.borrow_mut().take().quiet();
    let shown = Ptr::at(Region::Screen0);
    assert_eq!(
        chip.pixel(shown, 0, 0),
        3,
        "final branch copies screen column zero"
    );
    assert_eq!(
        chip.pixel(shown, 106, 0),
        4,
        "relative zero is never copied by the ordinary loop"
    );
    assert_eq!(chip.pixel(shown, 107, 0), 3);
    assert_eq!(chip.pixel(shown, 218, 93), 3);
    assert_eq!(chip.pixel(shown, 219, 93), 4);
}

#[test]
fn ending_songs_render_non_silent_audio_through_the_host_driver() {
    let Some(files) = files() else { return };
    let a = Assets::load(&files).quiet();
    let mut driver = crate::sound::AudioDriver::new(&files).quiet();
    let mut samples = vec![0.0f32; 22_050 * 2];
    for name in a.songs {
        driver.apply(&MedCall::Stop, &files);
        driver.apply(&MedCall::Load(name), &files);
        driver.apply(&MedCall::PlayBuffer, &files);
        driver.render(&mut samples, 22_050);
        assert!(
            samples.iter().any(|v| v.abs() > 0.001),
            "ending music produces samples"
        );
    }
    driver.apply(&MedCall::Stop, &files);
}

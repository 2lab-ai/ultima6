//! Spec 08 Order 3 / Acceptance 5 (slice K4) on the owner's files: the identity pack
//! (`crate::locale::identity`: every string the engine catalogues from his `u` translated as
//! itself, English grammar), installed on the game state before the first step as the page
//! installs a pack, turns `wide` on in `u`; the scripted opening, acknowledgments and creation,
//! the transfer without a save and the transfer of a synthetic Ultima V save then show every
//! tick's frame exactly as English, every catalogued string of `u` a hit (0 misses); every box it
//! draws is measured first and none falls back. A question of the gypsy that drops an answer key
//! refuses the pack. Skips without the archive; prints counts the runs made and pass/fail only.
#![allow(clippy::disallowed_methods)] // the tests read the zip

use std::rc::Rc;

use u6_testkit::Quiet;

use super::real::{Run, files, script, synthetic_u5_save, transfer_script, transfer_u5_script};
use crate::state::State;
use crate::ui::Language;

#[test]
fn the_identity_pack_shows_the_opening_creation_and_transfer_of_u_exactly_as_english() {
    let Some(files) = files() else { return };
    let pack = crate::locale::identity(&files).quiet();
    let mut with_save = files.clone();
    with_save.overlay(
        "ultima5",
        [("saved.gam".to_string(), synthetic_u5_save())].into(),
    );
    type Script = fn(&mut Run);
    let scripts: [(&str, &crate::files::Files, Script); 3] = [
        ("opening, acknowledgments and creation", &files, |r| {
            script(r, |_, _| {})
        }),
        ("transfer without a save", &files, |r| {
            transfer_script(r, |_, _| {})
        }),
        ("transfer of a synthetic Ultima V save", &with_save, |r| {
            transfer_u5_script(r, |_, _| {})
        }),
    ];
    for (name, f, play) in scripts {
        let run = |pack| {
            let mut r = Run::with_locale(f.clone(), pack).recording();
            play(&mut r);
            let counts = r.e.state().locale.program.get();
            (r.frames.take().quiet(), counts)
        };
        let (en, none) = run(None);
        let (id, [hits, misses]) = run(Some(pack.clone()));
        let differ = en.iter().zip(&id).filter(|(a, b)| a != b).count();
        eprintln!(
            "identity pack, u ({name}): ticks {} / {}, differing {differ}; u strings from the pack {hits}, misses {misses}",
            en.len(),
            id.len()
        );
        assert_eq!(none, [0, 0], "no pack: nothing counted");
        assert_eq!(en.len(), id.len(), "as many ticks");
        assert_eq!(differ, 0, "every frame as in English");
        assert!(hits > 0 && misses == 0, "every catalogued string a hit");
    }
    eprintln!("identity pack u identical to English: pass; misses 0: pass");
}

/// Slice K4 (the keys of a prompt) on the owner's `u`: a question of the gypsy whose English
/// shows an answer key standing alone, translated as itself but with that letter put as a Hangul
/// syllable (its codes kept), refuses the pack at install, naming the `u` part; translated as
/// itself, the pack installs. Skips without the archive; prints pass/fail only.
#[test]
fn a_question_of_the_owners_u_that_drops_an_answer_key_refuses_the_pack_naming_u() {
    use crate::locale::program::{Catalogue, Role, directives, keys_shown};
    use crate::locale::{Mismatch, Pack, Part, StringText, fingerprint, install};
    let Some(files) = files() else { return };
    let c = Catalogue::from_files(&files);
    let question = c
        .iter()
        .find(|(_, e)| e.role == Role::Gypsy && !keys_shown(&e.text, &e.keys).is_empty());
    let Some((hash, e)) = question else {
        panic!("a question of the gypsy shows its keys standing alone");
    };
    let key = keys_shown(&e.text, &e.keys)[0];
    let mut dropped = Vec::new();
    for (i, &b) in e.text.iter().enumerate() {
        let code = i > 0 && e.text[i - 1] == b'%';
        if b == key && !code {
            dropped.extend_from_slice("가".as_bytes());
        } else {
            dropped.push(b);
        }
    }
    assert!(
        directives(&dropped) == directives(&e.text),
        "its codes kept"
    );
    let with = |text: Vec<u8>| {
        let t = StringText {
            len: e.text.len() as u32,
            text,
        };
        let p = Pack::new(Language::Korean, "test/1", fingerprint([]), [])
            .and_then(|p| p.with_program([(hash, t)]))
            .quiet();
        let mut state = State::new_game(&files).quiet();
        install(&mut state, &files, Rc::new(p), None)
    };
    assert!(with(e.text.clone()).is_ok(), "as itself it installs");
    let refused = with(dropped);
    assert!(
        refused
            == Err(Mismatch {
                part: Part::Program
            }),
        "the key dropped refuses the pack"
    );
    eprintln!("a question of u without its answer key refuses the pack, naming u: pass");
}

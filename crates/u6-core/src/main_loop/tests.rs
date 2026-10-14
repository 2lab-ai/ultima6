//! Main-hunk helpers and the command loop on synthetic states, and the boot on the user's
//! files (skipped when the archive is absent; reads it, copies nothing).

use std::path::Path;

use super::*;
use crate::files::Files;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

#[allow(clippy::disallowed_methods)] // tests may read files
fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = u6_intake::intake(vec![u6_intake::InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// The boot on the user's files: 60 ticks reach the command prompt with only the two stub
/// points a boot passes (character creation 0x1351d8, and the combat song 0x110878 of the
/// combat start 0x121c64 for the disk's combat byte; the egg in the start window hatches
/// through the objects port 0x1319b2 and its placement through combat 0x121e88, so neither is
/// a stub point since milestone 2); the screen shows the window, the panel and the prompt.
#[test]
fn boot_reaches_the_prompt_with_two_stub_points() {
    let Some(files) = files() else { return };
    let mut e = crate::Engine::new(files).unwrap_or_else(|err| panic!("{err}"));
    for _ in 0..60 {
        e.tick(&[], 16);
    }
    let ids: Vec<&str> = e.stub_hits().keys().map(|k| &k[..8]).collect();
    assert_eq!(ids, ["0x110878", "0x1351d8"]);
    let s = e.state();
    assert_eq!(s.ui.text.current, crate::ui::WindowId::Messages);
    assert!(
        s.ui.messages.iter().any(|l| l.starts_with("Avatar:")),
        "{:?}",
        s.ui.messages
    );
    assert!(
        s.objects.draw.cell.iter().any(|&n| n != 0),
        "objects in the window"
    );
    assert_eq!(
        s.actors.objlist_tail[TAIL_CREATED], 1,
        "portrait byte of the new game"
    );
}

fn anim_state(tiles: &[u16]) -> State {
    let mut s = State::empty();
    s.tables.anim = u6_formats::animdata::AnimData {
        count: tiles.len() as u16,
        slots: tiles
            .iter()
            .map(|&t| u6_formats::animdata::AnimEntry {
                tile_to_animate: t,
                ..Default::default()
            })
            .collect(),
    };
    s.map.anim_enabled = vec![1; tiles.len()];
    s
}

#[test]
fn tile_animation_flag_goes_to_the_first_slot_of_the_tile() {
    let mut s = anim_state(&[10, 20, 20]);
    set_tile_animation(&mut s, 20, 2);
    assert_eq!(s.map.anim_enabled, [1, 2, 1]);
    set_tile_animation(&mut s, 30, 0);
    assert_eq!(s.map.anim_enabled, [1, 2, 1], "no slot, no write");
    s.tables.anim.count = 1;
    set_tile_animation(&mut s, 20, 0);
    assert_eq!(
        s.map.anim_enabled,
        [1, 2, 1],
        "slots past the count are not searched"
    );
}

#[test]
fn wind_stops_underground_and_changes_one_in_64_above() {
    let mut s = State::empty();
    s.map.centre.z = 2;
    s.actors.wind = 3;
    let rng = s.rng;
    wind_change(&mut s, 0);
    assert_eq!(
        (s.actors.wind, s.rng),
        (0xff, rng),
        "underground: calm, no draw"
    );
    // A seed whose first 0..=0x3f draw is 0: on the surface the wind turns (0..=8 minus 1).
    let seed = (1u32..1 << 20)
        .find(|&v| {
            let mut r = crate::Rng::new();
            r.set_state(v);
            r.range(0, 0x3f) == 0
        })
        .unwrap();
    let mut s = State::empty();
    s.rng.set_state(seed);
    let mut expect = s.rng;
    expect.range(0, 0x3f);
    let dir = (expect.range(0, 8) as u8).wrapping_sub(1);
    wind_change(&mut s, 0);
    assert_eq!((s.actors.wind, s.rng), (dir, expect));
    // With `calm` set (the sky bar's call) nothing is drawn.
    let mut s = State::empty();
    let rng = s.rng;
    wind_change(&mut s, 1);
    assert_eq!(s.rng, rng);
    // On level 3 with a calm wind: one draw, never a change.
    let mut s = State::empty();
    s.map.centre.z = 3;
    s.actors.wind = 0xff;
    s.rng.set_state(seed);
    wind_change(&mut s, 0);
    assert_eq!(s.actors.wind, 0xff);
}

#[test]
fn switch_leader_moves_the_centre_once() {
    let mut s = State::empty();
    s.actors.party.members[..2].copy_from_slice(&[1, 2]);
    s.actors.party.size = 2;
    s.objects.location[2] = u6_formats::position::Position { x: 40, y: 50, z: 0 };
    switch_leader(&mut s, 0);
    assert_eq!(s.actors.work.prompt, 0, "same index: nothing");
    switch_leader(&mut s, 1);
    assert_eq!(s.actors.party.leader_index, 1);
    assert_eq!((s.map.centre.x, s.map.centre.y), (40, 50));
    assert_eq!((s.gfx.window_shown, s.actors.work.prompt), (1, 1));
}

#[test]
fn volcano_probe_shakes_or_erupts_and_clears() {
    let mut s = State::empty();
    s.objects.draw.volcano_probe = 1;
    end_of_turn_volcano(&mut s);
    assert_eq!(s.objects.draw.volcano_probe, 1, "1 = still asking: no-op");
    s.objects.type_frame[300] = 0x133;
    s.objects.draw.volcano_probe = 300;
    let rng = s.rng;
    end_of_turn_volcano(&mut s);
    assert_eq!(
        (s.objects.draw.volcano_probe, s.rng),
        (0, rng),
        "0x133: a shake, no draw"
    );
    s.objects.type_frame[300] = 0xab;
    s.objects.draw.volcano_probe = 300;
    let mut expect = s.rng;
    expect.range(0, 1);
    end_of_turn_volcano(&mut s);
    assert_eq!(
        (s.objects.draw.volcano_probe, s.rng),
        (0, expect),
        "0xab: one draw"
    );
}

#[test]
fn moongates_follow_the_moon_phases_inside_the_window() {
    let mut s = State::empty();
    crate::objects::init_slots(&mut s.objects);
    s.map.window_origin = [0, 0];
    // Moonstone 0 at (10, 12, 0) in the window, moonstone 1 at (100, 12, 0) outside.
    s.actors.objlist_tail[0x2a..0x30].copy_from_slice(&[0, 10, 0, 12, 0, 0]);
    s.actors.objlist_tail[0x30..0x36].copy_from_slice(&[0, 100, 0, 12, 0, 0]);
    s.actors.objlist_tail[0x7d] = 3; // phase 1 below 15: the gates are up
    s.actors.objlist_tail[0x7f] = 15;
    moongates(&mut s);
    let gate = crate::objects::object_of_type_at(&s, 0x55, 10, 12);
    assert!(gate >= 0, "a red moongate at stone 0");
    assert_eq!(s.objects.tf(gate), 0x455);
    assert!(
        crate::objects::object_of_type_at(&s, 0x55, 100, 12) < 0,
        "outside the window"
    );
    assert_eq!(s.map.view_dirty, 1);
    s.map.view_dirty = 0;
    moongates(&mut s);
    assert_eq!(s.map.view_dirty, 0, "already there: no change");
    s.actors.objlist_tail[0x7d] = 15;
    moongates(&mut s);
    assert!(
        crate::objects::object_of_type_at(&s, 0x55, 10, 12) < 0,
        "both moons set: gone"
    );
    assert_eq!(s.map.view_dirty, 1);
}

/// Runs `dispatch(code)` with `keys` on a state with the test text layer and a party of one;
/// returns the code it gave and the state.
fn dispatched(code: u16, keys: &[crate::Key], mut s: State) -> (u16, State) {
    s.ui = test_support::test_ui();
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.objects.type_frame[1] = 0x17a;
    s.actors.movement_points[1] = 20;
    let out = std::rc::Rc::new(std::cell::Cell::new(None));
    let o = std::rc::Rc::clone(&out);
    let mut e = crate::Engine::with_game(s, Files::default(), move |st, io| async move {
        o.set(Some(commands::dispatch(&st, &io, code).await));
    });
    for &k in keys {
        e.tick(&[crate::InputEvent::Key(k)], 16);
    }
    for _ in 0..20 {
        e.tick(&[], 16);
    }
    let s = e.state().clone();
    (out.get().expect("dispatch returned"), s)
}

#[test]
fn exit_asks_until_y_or_n() {
    use crate::Key::Char;
    let (code, _) = dispatched(0x05, &[Char('x'), Char('n')], State::empty());
    assert_eq!(code, 0, "N: back to the game");
    let (code, _) = dispatched(0x05, &[Char('y')], State::empty());
    assert_eq!(code, 0x12d, "Y: the loop ends");
}

#[test]
fn pass_spends_the_leaders_points() {
    let (code, s) = dispatched(0x20, &[], State::empty());
    assert_eq!((code, s.actors.movement_points[1]), (0x20, 0));
}

/// Whether the message scroll shows the port's line, wrapped or not (the test window breaks
/// inside words, the game's at spaces, which it drops).
fn says_port_line(s: &State) -> bool {
    let line = String::from_utf8_lossy(commands::NOT_IMPLEMENTED);
    let line = line.trim_end();
    let m = &s.ui.messages;
    m.concat().contains(line) || m.join(" ").contains(line)
}

/// Every cut handler the loop still reaches (Rest, the debug teleport): the port's line, the
/// command state cleared, no movement points spent, no turn, and no key read (a queued key
/// stays queued).
#[test]
fn cut_commands_say_so_clear_the_command_and_take_no_turn() {
    let codes = [0x89, 0xd6];
    for combat in [0, 1] {
        for &code in &codes {
            let mut st = State::empty();
            st.actors.objlist_tail[commands::TAIL_SOLO] = 0xff; // party mode
            st.actors.objlist_tail[TAIL_COMBAT] = combat;
            st.actors.party.leader_index = 0;
            st.ui.input.targeting = 1;
            st.ui.input.target_pending = 1;
            let (got, s) = dispatched(code, &[], st);
            let what = format!("code {code:#x}, combat {combat}");
            assert_eq!(got, commands::NO_TURN, "{what}");
            assert_eq!(s.actors.movement_points[1], 20, "{what}");
            assert_eq!(
                (s.ui.input.target_pending, s.ui.input.targeting),
                (0, 0),
                "{what}"
            );
            assert_eq!(
                s.ui.input.target_command,
                crate::ui::globals::NO_COMMAND,
                "{what}"
            );
            assert!(says_port_line(&s), "{what}: {:?}", s.ui.messages);
        }
    }
}

/// Cast is magic's 0x11af10 (it reads its own input): with no spellbook readied it ends at once,
/// the turn taken, the command state cleared, no port line.
#[test]
fn cast_without_a_book_takes_the_turn() {
    let mut st = State::empty();
    st.actors.objlist_tail[commands::TAIL_SOLO] = 0xff;
    st.actors.party.leader_index = 0;
    let (code, s) = dispatched(0x82, &[], st);
    assert_eq!(code, 0x82);
    assert_eq!((s.ui.input.target_pending, s.ui.input.targeting), (0, 0));
    assert_eq!(s.ui.input.target_command, crate::ui::globals::NO_COMMAND);
    assert!(!says_port_line(&s));
}

/// Talk is not cut: it still waits for its target (a key that confirms nothing: "what?", which
/// the test text layer cannot print without the game's strings) and takes the turn.
#[test]
fn talk_still_reads_its_target() {
    let (code, s) = dispatched(0x83, &[crate::Key::Char('q')], State::empty());
    assert_eq!((code, s.actors.movement_points[1]), (0x83, 20));
    assert_eq!((s.ui.input.target_pending, s.ui.input.targeting), (0, 0));
    assert!(!says_port_line(&s));
}

/// Attack (0x124caa) given a key that confirms no target ("what?") takes its turn and leaves no
/// cursor up, so an arrow pressed after it walks the party (NICE-2: Attack used to
/// leave its cross-hair up).
#[test]
fn after_attack_an_arrow_walks() {
    use crate::Key;
    let mut s = State::empty();
    s.ui = test_support::test_ui();
    s.ui.tables.keymap[0x4e] = [0x14d; 4]; // the right arrow's direction code
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.objects.type_frame[1] = 0x17a;
    let walked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let w = std::rc::Rc::clone(&walked);
    let mut e = crate::Engine::with_game(s, Files::default(), move |st, io| async move {
        for _ in 0..3 {
            let key = crate::ui::wait::read_key(&st, &io).await;
            let code = commands::map_letter(&st, &io, key).await;
            let code = commands::dispatch(&st, &io, code).await;
            let targeting = st.borrow().ui.input.targeting;
            w.borrow_mut().push((code, targeting));
        }
    });
    for k in [Key::Char('A'), Key::Char('q'), Key::Right, Key::Right] {
        e.tick(&[crate::InputEvent::Key(k)], 16);
        for _ in 0..5 {
            e.tick(&[], 16);
        }
    }
    let got = walked.borrow().clone();
    assert_eq!(
        got,
        [(0x81, 0), (0x80, 0), (0x80, 0)],
        "A: the turn, cursor off; each arrow is a walk"
    );
}

/// On the user's files: `A` then a key that confirms no target runs the Attack command
/// (0x124caa: its lines, no cursor left up), and the arrow after it walks the party.
#[test]
fn real_attack_without_a_target_and_the_next_arrow_walks() {
    let Some(files) = files() else { return };
    let mut e = crate::Engine::with_seed(files, 7).unwrap_or_else(|err| panic!("{err}"));
    for _ in 0..60 {
        e.tick(&[], 16);
    }
    // The world turn after the Attack plays the start fight's hit flashes: enough ticks for it
    // to hand the turn back.
    let press = |e: &mut crate::Engine, k: crate::Key| {
        e.tick(&[crate::InputEvent::Key(k)], 16);
        for _ in 0..600 {
            e.tick(&[], 16);
        }
    };
    let lines = e.state().ui.messages.len();
    press(&mut e, crate::Key::Char('a'));
    press(&mut e, crate::Key::Char('q'));
    let s = e.state();
    assert!(!says_port_line(&s), "{:?}", s.ui.messages);
    assert!(s.ui.messages.len() > lines, "Attack printed its lines");
    assert_eq!((s.ui.input.targeting, s.ui.input.target_pending), (0, 0));
    let centre = s.map.centre;
    drop(s);
    press(&mut e, crate::Key::Right);
    assert_ne!(e.state().map.centre, centre, "the arrow walked");
}

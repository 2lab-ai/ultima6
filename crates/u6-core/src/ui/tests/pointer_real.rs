//! Real data: the whole game booted from the user's zip and played with the pointer only:
//! the Talk icon then party member 1 on the map, the Look icon then member 1, a map click that
//! walks, member 1 in the roster, the leader's inventory with the weapon off and on by clicks;
//! a press held on the map (a walk, its pace against the oracle's, a becalmed balloon, the exit
//! question). The command names and the member's name the scroll is checked for are read from
//! the user's files; the test prints counts and positions, not the game's text. Frames with the
//! pointer sprite go to /tmp/claude-501/m2-mouse/. Skips without the zip; the data is read only.
#![allow(clippy::disallowed_methods)] // the test reads the zip and writes the PNG receipts

use super::real_tests::{files, png};
use super::*;
use crate::objects::{ObjectId, READIED};
use crate::pacer::STEP_MS;
use u6_testkit::Quiet;

const OUT: &str = "/tmp/claude-501/m2-mouse";

/// Ticks of the hosts' step ([`STEP_MS`], spec 06) in `ms` of engine time, to the nearest.
fn ticks(ms: u32) -> usize {
    ((ms + STEP_MS / 2) / STEP_MS) as usize
}

/// The booted game, fed like the headless runner: ticks of [`STEP_MS`], an event once the
/// queue is empty, then idle ticks until the game waits again.
struct Play {
    e: Engine,
}

impl Play {
    fn boot(files: &Files) -> Play {
        let mut p = Play {
            e: Engine::game_first(files.clone(), 0).quiet(),
        };
        p.idle(ticks(3200));
        p
    }

    /// As [`Play::boot`] (seed 0, `game` at once), with a handle on the state to change it
    /// between ticks.
    fn boot_with_state(files: &Files) -> (Play, Rc<RefCell<State>>) {
        let mut state = State::new_game(files).quiet();
        state.rng.seed(0);
        let keep = Rc::new(RefCell::new(None));
        let k = Rc::clone(&keep);
        let e = Engine::with_game(state, files.clone(), move |st, io| {
            *k.borrow_mut() = Some(Rc::clone(&st));
            crate::main_loop::game_first(st, io)
        });
        let mut p = Play { e };
        p.idle(ticks(3200));
        let st = keep
            .borrow_mut()
            .take()
            .expect("the game future holds the state");
        (p, st)
    }

    /// As [`Play::boot_with_state`], then the oracle's quiet start (tools/oracle/pace.mjs): every
    /// actor outside the party gets status bit 0x10, so the world turn passes it over (0x1213de),
    /// and `b` breaks off the start fight: a step is the party's move alone.
    #[cfg(not(feature = "wide"))]
    fn boot_quiet(files: &Files) -> Play {
        let (mut p, st) = Play::boot_with_state(files);
        {
            let mut s = st.borrow_mut();
            let party: Vec<usize> = s.actors.party.active().map(|a| a.index()).collect();
            for a in (0..0x100).filter(|a| !party.contains(a)) {
                s.actors.attrs.status[a] |= 0x10;
            }
        }
        p.send(InputEvent::Key(Key::Char('b')));
        p.settle(ticks(32_000));
        p
    }

    /// Holds the left button three cells south of the hero for `ms` ([`ticks`]), then idles
    /// until the prompt is back; the cells walked.
    #[cfg(not(feature = "wide"))]
    fn held_steps(&mut self, ms: u32) -> u16 {
        let from = self.leader_at();
        let path = self.hold(near(0, 48), ticks(ms));
        self.settle(ticks(64_000));
        let at = self.leader_at();
        steps(from, &path).iter().sum::<u16>() + steps(*path.last().quiet(), &[at])[0]
    }

    /// The command loop waits for a command key and nothing is queued.
    fn at_prompt(&self) -> bool {
        self.e.io().pending_events() == 0 && self.e.state().main.reading == 1
    }

    /// Idle ticks until the prompt is up, at most `ticks`.
    fn settle(&mut self, ticks: usize) {
        for _ in 0..ticks {
            if self.at_prompt() {
                break;
            }
            self.idle(1);
        }
    }

    /// The name of command `code` as the game prints it (the table at A4-0x7c8e of the user's
    /// `game`, as 0x1041d8 reads it).
    fn command_name(&self, code: u32) -> String {
        let at = crate::image::A4.wrapping_sub(0x7c8e) + 4 * code;
        let name = crate::main_loop::pointed_string(self.e.io(), at);
        assert!(!name.is_empty(), "command {code:#x} has a name");
        String::from_utf8_lossy(&name).into_owned()
    }

    fn leader_at(&self) -> (u16, u16) {
        let p = self.e.state().leader_position().expect("a leader");
        (p.x, p.y)
    }

    /// Holds the left button at (x, y) for `ticks` ticks, then releases it; the leader's
    /// position after every tick of the hold and after the release.
    fn hold(&mut self, (x, y): (i16, i16), ticks: usize) -> Vec<(u16, u16)> {
        let mut at = Vec::with_capacity(ticks + 1);
        self.e
            .tick(&[InputEvent::Pointer { x, y, buttons: 1 }], STEP_MS);
        at.push(self.leader_at());
        for _ in 1..ticks {
            self.e.tick(&[], STEP_MS);
            at.push(self.leader_at());
        }
        self.e
            .tick(&[InputEvent::Pointer { x, y, buttons: 0 }], STEP_MS);
        at.push(self.leader_at());
        at
    }

    fn idle(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.e.tick(&[], STEP_MS);
        }
    }

    fn send(&mut self, ev: InputEvent) {
        self.send_all(&[ev]);
    }

    fn send_all(&mut self, evs: &[InputEvent]) {
        self.e.tick(evs, STEP_MS);
        for _ in 0..ticks(32_000) {
            if self.e.io().pending_events() == 0 {
                break;
            }
            self.e.tick(&[], STEP_MS);
        }
        assert!(self.e.io().pending_events() == 0, "the game took {evs:?}");
        self.idle(ticks(640));
    }

    /// A press and its release in one tick: the event wait samples the press, then the
    /// release, so the click map runs once (a longer hold repeats the map walk).
    fn click(&mut self, x: i16, y: i16) {
        self.send_all(&[
            InputEvent::Pointer { x, y, buttons: 1 },
            InputEvent::Pointer { x, y, buttons: 0 },
        ]);
    }

    fn keys(&mut self, text: &str) {
        for c in text.chars() {
            let k = if c == '\n' { Key::Enter } else { Key::Char(c) };
            self.send(InputEvent::Key(k));
        }
    }

    fn receipt(&self, name: &str) {
        std::fs::create_dir_all(OUT).quiet();
        let path = format!("{OUT}/{name}.png");
        std::fs::write(&path, png(self.e.frame())).quiet();
        println!("receipt {path}");
    }

    fn scroll(&self, n: usize) -> Vec<String> {
        let s = self.e.state();
        let m = &s.ui.messages;
        m[m.len().saturating_sub(n)..].to_vec()
    }

    /// The pixel centre of the view cell where party member `i` stands.
    fn member_cell(&self, i: usize) -> (i16, i16) {
        let s = self.e.state();
        let actor = s.actors.party.members[i];
        let p = s.objects.location(ObjectId(u16::from(actor))).quiet();
        let c = s.map.centre;
        near(
            (p.x as i16 - c.x as i16) * 16,
            (p.y as i16 - c.y as i16) * 16,
        )
    }

    fn weights(&self) -> (i16, i16) {
        let s = self.e.state();
        (s.inventory.equip_weight, s.inventory.carried_weight)
    }

    /// The pixel centre of list cell `k` of the inventory panel (4 per row from (0xf8, 0x20)).
    fn list_cell(k: usize) -> (i16, i16) {
        let k = k as i16;
        (0xf8 + (k % 4) * 16 + 8, 0x20 + (k / 4) * 16 + 8)
    }
}

/// Asserts the pointer sprite of the current shape is in the frame at the handler's pointer
/// (every set pixel in colour 16 + value); returns (shape, pixels checked).
fn sprite_shown(p: &Play) -> (u16, usize) {
    let s = p.e.state();
    let ptr = p.e.io().pointer();
    let sp = pointer::sprite(&s.ui.pointer, ptr).expect("the shapes are loaded");
    let fb = p.e.frame();
    let mut n = 0;
    for (r, row) in sp.rows.iter().enumerate() {
        for (c, &v) in row.iter().enumerate() {
            let (x, y) = (i32::from(sp.x) + c as i32, i32::from(sp.y) + r as i32);
            let (w, h) = (u6_render::WIDTH as i32, u6_render::HEIGHT as i32);
            if v != 0 && (0..w).contains(&x) && (0..h).contains(&y) {
                assert!(
                    fb.get(x as usize, y as usize) == Some(16 + v),
                    "a sprite pixel"
                );
                n += 1;
            }
        }
    }
    assert!(s.ui.pointer.shown && n > 0, "the pointer is shown");
    (s.ui.pointer.shape, n)
}

#[test]
fn the_game_played_with_the_pointer() {
    let Some(files) = files() else { return };
    let mut p = Play::boot(&files);
    assert!(p.e.state().ui.panel.mode == 0x91, "the roster at the start");
    sprite_shown(&p);
    p.receipt("boot");

    // The pointer over the map: the walk arrow toward it.
    let (x, y) = near(42, -38);
    p.send(InputEvent::Pointer { x, y, buttons: 0 });
    let (shape, _) = sprite_shown(&p);
    assert!(shape == 3, "north-east of the hero: the north-east arrow");
    p.receipt("arrow-ne");

    // Talk (the third icon), then member 1 on the map.
    p.click(48, 184);
    assert!(
        p.e.state().ui.input.target_pending == 1,
        "Talk waits for its target"
    );
    p.idle(2);
    assert!(
        sprite_shown(&p).0 == 1,
        "the cross-hair while a target is pending"
    );
    p.receipt("talk-target");
    let (dx, dy) = p.member_cell(1);
    p.click(dx, dy);
    for _ in 0..8 {
        if p.e.state().ui.input.line_input {
            break;
        }
        p.send(InputEvent::Key(Key::Space));
    }
    assert!(p.e.state().ui.input.line_input, "member 1 waits for words");
    p.receipt("talk-member1");
    let talk = p.scroll(12);
    let talk_name = p.command_name(0x83);
    assert!(
        talk.iter().any(|l| l.contains(&talk_name)),
        "the command's name is in the scroll"
    );
    p.keys("bye\n");
    p.idle(ticks(960));
    for _ in 0..4 {
        if p.e.state().main.reading == 1 {
            break;
        }
        p.send(InputEvent::Key(Key::Space));
    }

    // Look (the fourth icon): its name and the wait for its target; then member 1 on the map.
    let look_name = p.command_name(0x84);
    let member_name = p.e.state().actors.party.names[1].clone();
    p.click(64, 184);
    {
        let s = p.e.state();
        let line: String =
            s.ui.text
                .transcript_line
                .iter()
                .map(|&c| crate::ui::wide::latin_char(c))
                .collect();
        assert!(
            (s.ui.input.target_pending, s.ui.input.target_command) == (1, 0x84),
            "Look waits for its target"
        );
        assert!(line.contains(&look_name), "the command's name on the line");
    }
    p.receipt("look-target");
    let (dx, dy) = p.member_cell(1);
    p.click(dx, dy);
    for _ in 0..8 {
        if p.e.state().main.reading == 1 {
            break;
        }
        p.send(InputEvent::Key(Key::Space));
    }
    let look = p.scroll(8);
    assert!(
        look.iter().any(|l| l.contains(&look_name)),
        "Look in the scroll"
    );
    assert!(
        look.iter().any(|l| l.contains(&member_name)),
        "Look names member 1"
    );
    assert!(p.e.state().ui.input.target_pending == 0);
    p.receipt("look-member1");

    // A map cell south of the hero: one step south.
    let before = p.e.state().map.centre;
    let (x, y) = near(0, 16);
    p.click(x, y);
    let after = p.e.state().map.centre;
    assert!(
        (after.x, after.y) == (before.x, before.y + 1),
        "one step south"
    );
    p.receipt("walked-south");

    // Member 1 in the roster: the stats.
    p.click(0xe0, 0x34);
    {
        let s = p.e.state();
        assert!(
            (s.ui.panel.mode, s.ui.panel.member) == (0x90, 1),
            "member 1's stats"
        );
    }
    p.receipt("member1-stats");

    // The roster icon, then the hero's figure: the hero's inventory.
    p.click(0xc8, 0x60);
    assert!(p.e.state().ui.panel.mode == 0x91);
    p.click(0xb8, 0x20);
    {
        let s = p.e.state();
        assert!(
            (s.ui.panel.mode, s.ui.panel.member) == (0x92, 0),
            "the hero's inventory"
        );
    }
    let sword = p.e.state().inventory.equip(2);
    assert!(sword > 1, "a weapon in the hero's hand");
    let held = p.weights();
    p.receipt("hero-inventory");

    // The sword in hand: off (Unready, 0xa8).
    p.click(0xb8, 0x30);
    let off = p.weights();
    let k = {
        let s = p.e.state();
        assert!(s.inventory.equip(2) == 0, "the hand is empty");
        assert!(s.objects.place(sword) != READIED);
        s.inventory
            .cells
            .iter()
            .position(|&c| c == sword)
            .expect("the sword in the list")
    };
    p.receipt("sword-off");

    // The sword in the list: on again (Ready, 0xa7).
    let (x, y) = Play::list_cell(k);
    p.click(x, y);
    let on = p.weights();
    assert!(
        p.e.state().inventory.equip(2) == sword,
        "the sword in hand again"
    );
    assert!(on == held, "the weights as before the Unready");
    assert!(off != held, "the Unready changed the weights");
    sprite_shown(&p);
    p.receipt("sword-on");
    println!("stubs: {:?}", p.e.stub_hits().keys().collect::<Vec<_>>());
}

/// Cells moved between consecutive positions (the larger axis).
fn steps(from: (u16, u16), path: &[(u16, u16)]) -> Vec<u16> {
    std::iter::once(from)
        .chain(path.iter().copied())
        .collect::<Vec<_>>()
        .windows(2)
        .map(|w| w[0].0.abs_diff(w[1].0).max(w[0].1.abs_diff(w[1].1)))
        .collect()
}

#[test]
fn a_held_press_walks_a_cell_a_step_and_stops_at_the_release() {
    let Some(files) = files() else { return };
    let mut p = Play::boot(&files);
    assert!(p.at_prompt(), "boot reaches the prompt");
    // 15 s: past the start fight's first world turn (its NPCs' turns take seconds) and a repeat.
    let held = ticks(15_000);
    // Three cells away in each direction: the first hold that walks more than one cell.
    for (dx, dy) in [
        (0, 3),
        (-3, 0),
        (0, -3),
        (3, 0),
        (3, 3),
        (-3, 3),
        (-3, -3),
        (3, -3),
    ] {
        let from = p.leader_at();
        let path = p.hold(near(16 * dx, 16 * dy), held);
        p.settle(ticks(64_000));
        let st = steps(from, &path);
        let moved: u16 = st.iter().sum();
        if moved < 2 {
            continue;
        }
        assert!(st.iter().all(|&d| d <= 1), "one cell a step");
        assert!(
            Some(&p.leader_at()) == path.last(),
            "no step after the release"
        );
        assert!(p.at_prompt(), "the prompt again");
        return;
    }
    panic!("no held walk got past one cell");
}

/// The original's held walks three cells south of the hero, measured in the oracle
/// (tools/oracle/pace.mjs, game-first disks, 3 to 6 boots): (PAL frames held, the cells walked
/// in most trials). A press of 5 or 10 frames counts when the game sampled it; 200 frames = the
/// steps by frame 200 of the 300-frame holds. Quiet = every actor outside the party passed over
/// and the fight broken off; the start = the throne-room fight as booted, whose first world
/// turn takes 420..470 frames.
#[cfg(not(feature = "wide"))]
const ORACLE_QUIET: [(u32, u16); 5] = [(5, 1), (10, 1), (25, 1), (50, 2), (200, 6)];
#[cfg(not(feature = "wide"))]
const ORACLE_START: [(u32, u16); 4] = [(5, 1), (10, 1), (25, 1), (50, 1)];

/// The port's cells for the oracle's holds (a PAL frame = 20 ms), each from a fresh boot.
#[cfg(not(feature = "wide"))]
fn port_counts(boot: impl Fn() -> Play, oracle: &[(u32, u16)]) -> Vec<(u32, u16, u16)> {
    oracle
        .iter()
        .map(|&(frames, orig)| (frames, orig, boot().held_steps(frames * 20)))
        .collect()
}

/// Against the oracle's runs: Original only (spec 13 Tests).
#[cfg(not(feature = "wide"))]
#[test]
fn a_held_press_walks_at_the_originals_pace() {
    let Some(files) = files() else { return };
    for (name, counts) in [
        (
            "quiet",
            port_counts(|| Play::boot_quiet(&files), &ORACLE_QUIET),
        ),
        (
            "start fight",
            port_counts(|| Play::boot(&files), &ORACLE_START),
        ),
    ] {
        println!(
            "held walk, {name}: (PAL frames held, the original's cells, the port's) {counts:?}"
        );
        for &(frames, orig, port) in &counts {
            if frames * 20 < crate::ui::target::HELD_WALK_MS {
                // Shorter than a step (100 ms included): one cell, as the original.
                assert!(port == orig, "{name}: a press of {frames} frames");
            } else {
                assert!(
                    port.abs_diff(orig) <= 1,
                    "{name}: held {frames} frames, the original's {orig} cells within one"
                );
            }
        }
    }
}

#[test]
fn a_held_press_in_a_calm_balloon_does_not_hang() {
    let Some(files) = files() else { return };
    let (mut p, st) = Play::boot_with_state(&files);
    assert!(p.at_prompt(), "boot reaches the prompt");
    {
        // The leader becomes the balloon (type 0x1a7, 0x11d4ee); no other actor takes a turn
        // (the scheduler skips status bit 0x10), so no effect of theirs plays between the tries.
        let mut s = st.borrow_mut();
        let l = s.actors.party.leader().expect("a leader").index();
        s.objects.type_frame[l] = (s.objects.type_frame[l] & !0x3ff) | 0x1a7;
        let party: Vec<usize> = s.actors.party.active().map(|a| a.index()).collect();
        for a in (0..0x100).filter(|a| !party.contains(a)) {
            s.actors.attrs.status[a] |= 0x10;
        }
    }
    let from = p.leader_at();
    let lines = |p: &Play| p.e.state().ui.messages.len();
    let mut grown = Vec::new();
    let (x, y) = near(48, 0);
    for t in 0..13 {
        // Calm (-1) before every tick: the clock's wind changes (0x10368a) do not end the try.
        st.borrow_mut().actors.wind = 0xff;
        let before = lines(&p);
        let buttons = u8::from(t < 12);
        p.e.tick(&[InputEvent::Pointer { x, y, buttons }], STEP_MS);
        grown.push(lines(&p) - before);
        assert!(p.leader_at() == from, "no wind: no move (tick {t})");
    }
    p.settle(ticks(9600));
    // Each try prints the same lines: the press's try is the first tick's growth.
    let per_try = grown.iter().copied().find(|&g| g > 0).unwrap_or(0);
    let total: usize = grown.iter().sum();
    assert!(
        per_try > 0 && total.is_multiple_of(per_try),
        "the same lines each try"
    );
    assert!(total / per_try <= 13, "one try a frame at most");
    assert!(p.at_prompt(), "the prompt again after the release");
}

#[test]
fn a_held_map_press_during_the_exit_question_does_not_hang() {
    let Some(files) = files() else { return };
    let mut p = Play::boot(&files);
    assert!(p.at_prompt(), "boot reaches the prompt");
    let from = p.leader_at();
    // Ctrl-Q (0x11): the exit question, read with 0x10790a until Y or N.
    p.send(InputEvent::Key(Key::Char('\u{11}')));
    assert!(!p.at_prompt(), "the exit question waits");
    let path = p.hold(near(48, 0), ticks(192));
    p.idle(ticks(320));
    assert!(
        path.iter().all(|&a| a == from),
        "the question takes no walk"
    );
    assert!(!p.at_prompt(), "still asking after the release");
    p.send(InputEvent::Key(Key::Char('n')));
    assert!(p.at_prompt(), "N: the prompt again");
    assert!(p.leader_at() == from);
}

/// Spec 22 receipt: Talk to member 1 by the pointer, then press its Name button, its Job button and
/// a keyword the scroll shows in the alert colour; the same run with the pointer moved to the same
/// places and the words typed instead (each word and Enter in one tick, as a press sends them) ends
/// each answer with the same words, scroll, frame and state. At each read the two buttons are drawn
/// again, their row the same pixels (and HD sources) as at the first: the row the read before
/// filled with the paper on the real paper map is all paper again (gpt-a SHOULD 1), after the
/// keyword's answer too (gpt a1-1). Counts and equality only; prints whether it ran; with
/// `U6_TALK_FRAMES` set, frames of the buttons and of a hovered keyword go there.
#[test]
fn a_real_talk_answers_a_press_as_the_word_typed() {
    let Some(files) = files() else {
        println!("real talk receipt: skipped (no game files)");
        return;
    };
    let begin = || {
        let mut p = Play::boot(&files);
        p.click(48, 184);
        let (dx, dy) = p.member_cell(1);
        p.click(dx, dy);
        p.words();
        p
    };
    let (mut press, mut typed) = (begin(), begin());
    let rest = press.e.state().ui.pointer.at;
    assert!(
        press.e.state().ui.input.line_input,
        "member 1 waits for words"
    );
    let (n, pixels) = press.buttons();
    assert_eq!(n, 2, "the Job and Name buttons");
    println!("buttons {n}, drawn pixels {pixels}");
    let first_row = press.button_row();
    press.frame("buttons");
    let button = |p: &Play, i: usize| {
        let s = p.e.state();
        let b = &s.ui.talk.buttons[i];
        let w = s.ui.text.window(WindowId::Status);
        let col = usize::from(b.col) + b.cells.len() / 2;
        let row = usize::from(w.y) + usize::from(w.height) - 1;
        (((col * CELL) + 4) as i16, ((row * CELL) + 4) as i16)
    };
    // The next read after an answer: the words read again, the two buttons back, and, the
    // pointer back where the read began (off the buttons: resting on one it shows it inverse,
    // Contract 6), their row as at the first read.
    let again = |press: &mut Play, typed: &mut Play, after: &str| {
        assert!(
            press.e.state().ui.input.line_input,
            "{after}: the words read again"
        );
        assert_eq!(press.buttons().0, 2, "{after}: the buttons again");
        for p in [&mut *press, &mut *typed] {
            p.send(InputEvent::Pointer {
                x: rest.0,
                y: rest.1,
                buttons: 0,
            });
        }
        assert!(
            press.button_row() == first_row,
            "{after}: their row as at the first read"
        );
    };
    let mut answers = 0;
    // Name, then Job after a letter typed: the press takes the letter back first.
    for (i, before) in [(1, 0), (0, 1)] {
        let xy = button(&press, i);
        let word = press.e.state().ui.talk.buttons[i].word.clone();
        for p in [&mut press, &mut typed] {
            for _ in 0..before {
                p.send(InputEvent::Key(Key::Char('x')));
            }
        }
        press.answer(xy, None);
        typed.answer(xy, Some((before, &word)));
        press.same_as(&typed, &word);
        answers += 1;
        again(&mut press, &mut typed, "a button's answer");
    }
    let span = {
        let s = press.e.state();
        s.ui.text.keys.spans.first().cloned()
    };
    let span = span.expect("a keyword in the alert colour after the answers");
    let w = *press.e.state().ui.text.window(WindowId::Messages);
    let (col, row) = (usize::from(w.x + span.col), usize::from(w.y + span.row));
    let xy = (((col * CELL) + 4) as i16, ((row * CELL) + 4) as i16);
    for p in [&mut press, &mut typed] {
        p.send(InputEvent::Pointer {
            x: xy.0,
            y: xy.1,
            buttons: 0,
        });
    }
    assert!(
        matches!(press.e.state().ui.talk.hot, Some(talk::Target::Key { .. })),
        "the keyword under the pointer drawn inverse"
    );
    press.frame("keyword-hover");
    let word = press.e.state().ui.text.keys.word(span.row, span.col);
    press.answer(xy, None);
    typed.answer(xy, Some((0, &word)));
    press.same_as(&typed, &word);
    answers += 1;
    // gpt a1-1: the same after the keyword's answer.
    again(&mut press, &mut typed, "the keyword's answer");
    println!(
        "presses answered as typed: {answers} (keyword of {} cells, spans shown {})",
        span.cells.len(),
        press.e.state().ui.text.keys.spans.len()
    );
    println!("real talk receipt: ran");
}

impl Play {
    /// Pages on (Space) until the conversation waits for the player's words.
    fn words(&mut self) {
        for _ in 0..16 {
            if self.e.state().ui.input.line_input {
                break;
            }
            self.send(InputEvent::Key(Key::Space));
        }
    }

    /// The buttons shown, and the pixels of the command colour on their row.
    fn buttons(&self) -> (usize, usize) {
        let s = self.e.state();
        let command = s.ui.tables.colours.command;
        let w = s.ui.text.window(WindowId::Status);
        let row = usize::from(w.y) + usize::from(w.height) - 1;
        let x0 = usize::from(w.x) * CELL;
        let fb = self.e.frame();
        let drawn = (row * CELL..(row + 1) * CELL)
            .flat_map(|y| (x0..x0 + usize::from(w.width) * CELL).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.get(x, y) == Some(command))
            .count();
        (s.ui.talk.buttons.len(), drawn)
    }

    /// The buttons' row (the Status window's last) as the game drew it (no pointer sprite):
    /// each pixel and its HD source.
    fn button_row(&self) -> Vec<(Option<u8>, Option<u6_render::Src>)> {
        let s = self.e.state();
        let w = s.ui.text.window(WindowId::Status);
        let row = usize::from(w.y) + usize::from(w.height) - 1;
        let x0 = usize::from(w.x) * CELL;
        let fb = self.e.published_frame();
        (row * CELL..(row + 1) * CELL)
            .flat_map(|y| (x0..x0 + usize::from(w.width) * CELL).map(move |x| (x, y)))
            .map(|(x, y)| (fb.get(x, y), fb.src_at(x, y)))
            .collect()
    }

    /// The pointer moved to `xy`, then a press and release there, or `back` Backspaces, `word`
    /// and Enter typed in one tick; then paged on to the next words.
    fn answer(&mut self, (x, y): (i16, i16), typed: Option<(usize, &[u8])>) {
        self.send(InputEvent::Pointer { x, y, buttons: 0 });
        match typed {
            None => self.click(x, y),
            Some((back, w)) => {
                let mut keys = vec![InputEvent::Key(Key::Backspace); back];
                keys.extend(String::from_utf8_lossy(w).chars().map(|c| match c {
                    ' ' => InputEvent::Key(Key::Space),
                    c => InputEvent::Key(Key::Char(c)),
                }));
                keys.push(InputEvent::Key(Key::Enter));
                self.send_all(&keys);
            }
        }
        self.words();
    }

    /// The answer `self` (pressed) got equals `typed`'s, and the player's words were `word`.
    fn same_as(&self, typed: &Play, word: &[u8]) {
        let (a, b) = (self.e.state(), typed.e.state());
        assert!(a.converse.input == word, "the press typed its word");
        assert!(a.converse.input == b.converse.input, "the same words");
        assert!(a.ui.messages == b.ui.messages, "the same scroll");
        assert!(
            self.e.frame().pixels() == typed.e.frame().pixels(),
            "the same frame"
        );
        assert!(*a == *b, "the same state");
    }

    /// Writes the frame to `$U6_TALK_FRAMES/<build>-<name>.png` when that is set.
    fn frame(&self, name: &str) {
        let Some(dir) = std::env::var_os("U6_TALK_FRAMES") else {
            return;
        };
        let build = if cfg!(feature = "wide") {
            "wide"
        } else {
            "original"
        };
        let path = std::path::Path::new(&dir).join(format!("{build}-{name}.png"));
        std::fs::create_dir_all(&dir).quiet();
        std::fs::write(&path, png(self.e.frame())).quiet();
        println!("frame {}", path.display());
    }
}

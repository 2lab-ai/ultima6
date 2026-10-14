//! Real data: the whole game booted from the user's zip and played with the pointer only:
//! the Talk icon then party member 1 on the map, the Look icon then member 1, a map click that
//! walks, member 1 in the roster, the leader's inventory with the weapon off and on by clicks;
//! a press held on the map (a walk, a becalmed balloon, the exit question). The command names
//! and the member's name the scroll is checked for are read from the user's files; the test
//! prints counts and positions, not the game's text. Frames with the pointer
//! sprite go to /tmp/claude-501/m2-mouse/. Skips without the zip; the data is read only.
#![allow(clippy::disallowed_methods)] // the test reads the zip and writes the PNG receipts

use super::real_tests::{files, png};
use super::*;
use crate::objects::{ObjectId, READIED};

const OUT: &str = "/tmp/claude-501/m2-mouse";

/// The booted game, fed like the headless runner: an event once the queue is empty, then idle
/// ticks until the game waits again.
struct Play {
    e: Engine,
}

impl Play {
    fn boot(files: &Files) -> Play {
        let mut p = Play {
            e: Engine::game_first(files.clone(), 0).unwrap_or_else(|e| panic!("{e}")),
        };
        p.idle(200);
        p
    }

    /// As [`Play::boot`] (seed 0, `game` at once), with a handle on the state to change it
    /// between ticks.
    fn boot_with_state(files: &Files) -> (Play, Rc<RefCell<State>>) {
        let mut state = State::new_game(files).unwrap_or_else(|e| panic!("{e}"));
        state.rng.seed(0);
        let keep = Rc::new(RefCell::new(None));
        let k = Rc::clone(&keep);
        let e = Engine::with_game(state, files.clone(), move |st, io| {
            *k.borrow_mut() = Some(Rc::clone(&st));
            crate::main_loop::game_first(st, io)
        });
        let mut p = Play { e };
        p.idle(200);
        let st = keep
            .borrow_mut()
            .take()
            .expect("the game future holds the state");
        (p, st)
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
        self.e.tick(&[InputEvent::Pointer { x, y, buttons: 1 }], 16);
        at.push(self.leader_at());
        for _ in 1..ticks {
            self.e.tick(&[], 16);
            at.push(self.leader_at());
        }
        self.e.tick(&[InputEvent::Pointer { x, y, buttons: 0 }], 16);
        at.push(self.leader_at());
        at
    }

    fn idle(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.e.tick(&[], 16);
        }
    }

    fn send(&mut self, ev: InputEvent) {
        self.send_all(&[ev]);
    }

    fn send_all(&mut self, evs: &[InputEvent]) {
        self.e.tick(evs, 16);
        for _ in 0..2000 {
            if self.e.io().pending_events() == 0 {
                break;
            }
            self.e.tick(&[], 16);
        }
        assert_eq!(self.e.io().pending_events(), 0, "the game took {evs:?}");
        self.idle(40);
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
        std::fs::create_dir_all(OUT).unwrap();
        let path = format!("{OUT}/{name}.png");
        std::fs::write(&path, png(self.e.frame())).unwrap();
        println!("receipt {path}");
    }

    fn scroll(&self, n: usize) -> Vec<String> {
        let s = self.e.state();
        let m = &s.ui.messages;
        m[m.len().saturating_sub(n)..].to_vec()
    }

    /// The pixel centre of the window cell where party member `i` stands.
    fn member_cell(&self, i: usize) -> (i16, i16) {
        let s = self.e.state();
        let actor = s.actors.party.members[i];
        let p = s.objects.location(ObjectId(u16::from(actor))).unwrap();
        let c = s.map.centre;
        let col = p.x as i16 - c.x as i16 + 5;
        let row = p.y as i16 - c.y as i16 + 5;
        (col * 16 + 8, row * 16 + 8)
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
            if v != 0 && (0..320).contains(&x) && (0..200).contains(&y) {
                assert_eq!(
                    fb.get(x as usize, y as usize),
                    Some(16 + v),
                    "sprite pixel ({x}, {y})"
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
    assert_eq!(p.e.state().ui.panel.mode, 0x91, "the roster at the start");
    let (shape, n) = sprite_shown(&p);
    println!(
        "boot: pointer {:?} shape {shape}, {n} sprite pixels",
        p.e.io().pointer()
    );
    p.receipt("boot");

    // The pointer over the map: the walk arrow toward it.
    p.send(InputEvent::Pointer {
        x: 130,
        y: 50,
        buttons: 0,
    });
    let (shape, _) = sprite_shown(&p);
    assert_eq!(shape, 3, "north-east of the Avatar: the north-east arrow");
    p.receipt("arrow-ne");

    // Talk (the third icon), then member 1 on the map.
    p.click(48, 184);
    assert_eq!(
        p.e.state().ui.input.target_pending,
        1,
        "Talk waits for its target"
    );
    p.idle(2);
    assert_eq!(
        sprite_shown(&p).0,
        1,
        "the cross-hair while a target is pending"
    );
    p.receipt("talk-target");
    let (dx, dy) = p.member_cell(1);
    println!("member 1 at pixel ({dx}, {dy})");
    p.click(dx, dy);
    for _ in 0..8 {
        if p.e.state().ui.input.line_input {
            break;
        }
        p.send(InputEvent::Key(Key::Space));
    }
    assert!(p.e.state().ui.input.line_input, "member 1 waits for words");
    p.receipt("talk-dupre");
    let talk = p.scroll(12);
    println!("talk: {} scroll lines", talk.len());
    let talk_name = p.command_name(0x83);
    assert!(
        talk.iter().any(|l| l.contains(&talk_name)),
        "the command's name is in the scroll"
    );
    p.keys("bye\n");
    p.idle(60);
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
        let line = String::from_utf8_lossy(&s.ui.text.transcript_line).into_owned();
        assert_eq!(
            (s.ui.input.target_pending, s.ui.input.target_command),
            (1, 0x84),
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
    println!("look: {} scroll lines", look.len());
    assert!(
        look.iter().any(|l| l.contains(&look_name)),
        "Look in the scroll"
    );
    assert!(
        look.iter().any(|l| l.contains(&member_name)),
        "Look names member 1"
    );
    assert_eq!(p.e.state().ui.input.target_pending, 0);
    p.receipt("look-dupre");

    // A map cell south of the Avatar: one step south.
    let before = p.e.state().map.centre;
    p.click(88, 104);
    let after = p.e.state().map.centre;
    println!("walk: centre {before:?} -> {after:?}");
    assert_eq!(
        (after.x, after.y),
        (before.x, before.y + 1),
        "one step south"
    );
    p.receipt("walked-south");

    // Member 1 in the roster: the stats.
    p.click(0xe0, 0x34);
    {
        let s = p.e.state();
        assert_eq!(
            (s.ui.panel.mode, s.ui.panel.member),
            (0x90, 1),
            "member 1's stats"
        );
    }
    p.receipt("dupre-stats");

    // The roster icon, then the Avatar's figure: the Avatar's inventory.
    p.click(0xc8, 0x60);
    assert_eq!(p.e.state().ui.panel.mode, 0x91);
    p.click(0xb8, 0x20);
    {
        let s = p.e.state();
        assert_eq!(
            (s.ui.panel.mode, s.ui.panel.member),
            (0x92, 0),
            "the Avatar's inventory"
        );
    }
    let sword = p.e.state().inventory.equip(2);
    assert!(sword > 1, "a weapon in the Avatar's hand");
    let held = p.weights();
    p.receipt("avatar-inventory");

    // The sword in hand: off (Unready, 0xa8).
    p.click(0xb8, 0x30);
    let off = p.weights();
    let k = {
        let s = p.e.state();
        assert_eq!(s.inventory.equip(2), 0, "the hand is empty");
        assert_ne!(s.objects.place(sword), READIED);
        s.inventory
            .cells
            .iter()
            .position(|&c| c == sword)
            .expect("the sword in the list")
    };
    println!("Unready by click: equip/carried tenths {held:?} -> {off:?}, list cell {k}");
    p.receipt("sword-off");

    // The sword in the list: on again (Ready, 0xa7).
    let (x, y) = Play::list_cell(k);
    p.click(x, y);
    let on = p.weights();
    assert_eq!(
        p.e.state().inventory.equip(2),
        sword,
        "the sword in hand again"
    );
    assert_eq!(on, held);
    println!("Ready by click: equip/carried tenths {off:?} -> {on:?}");
    sprite_shown(&p);
    p.receipt("sword-on");
    println!("stubs: {:?}", p.e.stub_hits());
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
fn a_held_press_walks_a_step_a_frame_and_stops_at_the_release() {
    let Some(files) = files() else { return };
    let mut p = Play::boot(&files);
    assert!(p.at_prompt(), "boot reaches the prompt");
    const HELD: usize = 90;
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
        let path = p.hold((88 + 16 * dx, 88 + 16 * dy), HELD);
        p.idle(400);
        let st = steps(from, &path);
        let moved: u16 = st.iter().sum();
        let walked: Vec<usize> = (0..st.len()).filter(|&t| st[t] != 0).collect();
        println!("held {HELD} ticks toward ({dx}, {dy}): {moved} cells, at ticks {walked:?}");
        if moved < 2 {
            continue;
        }
        assert!(
            st.iter().all(|&d| d <= 1),
            "one step a frame at most: {st:?}"
        );
        assert_eq!(
            Some(&p.leader_at()),
            path.last(),
            "no step after the release"
        );
        assert!(p.at_prompt(), "the prompt again");
        return;
    }
    panic!("no held walk got past one cell");
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
    let (x, y) = (88 + 48, 88);
    for t in 0..13 {
        // Calm (-1) before every tick: the clock's wind changes (0x10368a) do not end the try.
        st.borrow_mut().actors.wind = 0xff;
        let before = lines(&p);
        let buttons = u8::from(t < 12);
        p.e.tick(&[InputEvent::Pointer { x, y, buttons }], 16);
        grown.push(lines(&p) - before);
        assert_eq!(p.leader_at(), from, "no wind: no move (tick {t})");
    }
    p.settle(600);
    // Each try prints the same lines: the press's try is the first tick's growth.
    let per_try = grown[0];
    let total: usize = grown.iter().sum();
    println!("calm balloon, held 12 ticks: scroll lines per tick {grown:?}");
    assert!(per_try > 0 && total.is_multiple_of(per_try), "{grown:?}");
    assert!(total / per_try <= 13, "one try a frame at most: {grown:?}");
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
    let path = p.hold((88 + 48, 88), 12);
    p.idle(20);
    assert!(
        path.iter().all(|&a| a == from),
        "the question takes no walk"
    );
    assert!(!p.at_prompt(), "still asking after the release");
    p.send(InputEvent::Key(Key::Char('n')));
    assert!(p.at_prompt(), "N: the prompt again");
    assert_eq!(p.leader_at(), from);
}

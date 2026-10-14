//! Real data: the whole game booted from the user's zip and played with the pointer only:
//! the Talk icon then Dupre on the map, the Look icon then Dupre, a map click that walks, Dupre in the
//! roster, the Avatar's inventory with the sword off and on by clicks. Frames with the pointer
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

    // Talk (the third icon), then Dupre on the map.
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
    println!("Dupre at pixel ({dx}, {dy})");
    p.click(dx, dy);
    for _ in 0..8 {
        if p.e.state().ui.input.line_input {
            break;
        }
        p.send(InputEvent::Key(Key::Space));
    }
    assert!(p.e.state().ui.input.line_input, "Dupre waits for words");
    p.receipt("talk-dupre");
    let talk = p.scroll(12);
    println!("talk: {talk:#?}");
    assert!(talk.iter().any(|l| l.contains("Talk-")), "{talk:?}");
    p.keys("bye\n");
    p.idle(60);
    for _ in 0..4 {
        if p.e.state().main.reading == 1 {
            break;
        }
        p.send(InputEvent::Key(Key::Space));
    }

    // Look (the fourth icon): "Look-" and the wait for its target; then Dupre on the map.
    p.click(64, 184);
    {
        let s = p.e.state();
        let line = String::from_utf8_lossy(&s.ui.text.transcript_line).into_owned();
        assert_eq!(
            (s.ui.input.target_pending, s.ui.input.target_command),
            (1, 0x84),
            "Look waits for its target"
        );
        assert!(line.contains("Look-"), "{line:?}");
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
    println!("look: {look:?}");
    assert!(look.iter().any(|l| l.contains("Look-")), "{look:?}");
    assert!(
        look.iter().any(|l| l.contains("Dupre")),
        "Look describes Dupre: {look:?}"
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

    // Dupre in the roster: his stats.
    p.click(0xe0, 0x34);
    {
        let s = p.e.state();
        assert_eq!(
            (s.ui.panel.mode, s.ui.panel.member),
            (0x90, 1),
            "Dupre's stats"
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

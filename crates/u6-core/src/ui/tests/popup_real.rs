//! Real data (spec 17 Presentation, unit M3): the whole game booted from the user's zip with the
//! context menu on, the leader put beside a closed door: a right press on the door and one on
//! bare ground open the menu in the frame (its text area paper, ink and the heading's colour,
//! the published frame outside its box, the game's button untouched); Use chosen by a left press
//! opens the door; Go here, chosen by its row, calls unit M4's seam with the cell. Frames of the
//! open menu (the frame, and the HD compositor's at 4×) go to the directory
//! `U6_FRAMES_DIR` names, which must lie outside the repository (they show the user's game); the
//! test prints counts only. Skips without the zip; the data is read only.
#![allow(clippy::disallowed_methods)] // the test reads the zip and writes the PNG receipts

use super::real_tests::{files, png, png_rgba};
use super::*;
use crate::engine::{POINTER_LEFT, POINTER_RIGHT};
use crate::hd::HdView;
use crate::objects::Slot;
use crate::objects::commands::real_data::closed_door;
use crate::pacer::STEP_MS;
use crate::ui::menu;
use crate::ui::popup::{RowKind, View};
use u6_render::hd::Scale;
use u6_testkit::Quiet;

/// The build's name in the frames' names.
const BUILD: &str = if cfg!(feature = "wide") {
    "wide"
} else {
    "original"
};

/// The booted game with a handle on its state, fed as the hosts feed it: ticks of [`STEP_MS`].
struct Play {
    e: Engine,
    st: Rc<RefCell<State>>,
}

impl Play {
    /// The game (seed 0, `game` at once) with the context menu on and the source planes (as the
    /// web build starts it), every actor outside the party passed over by the world turn (status
    /// bit 0x10, as the oracle's quiet start) and the start fight broken off (`b`): at the prompt.
    fn boot(files: &Files) -> Play {
        let mut state = State::new_game(files).quiet();
        state.rng.seed(0);
        let keep = Rc::new(RefCell::new(None));
        let k = Rc::clone(&keep);
        let mut e = Engine::with_game(state, files.clone(), move |st, io| {
            *k.borrow_mut() = Some(Rc::clone(&st));
            crate::main_loop::game_first(st, io)
        });
        e.enable_src_plane();
        e.set_context_menu(true);
        let st = keep
            .borrow_mut()
            .take()
            .expect("the game future holds the state");
        let mut p = Play { e, st };
        p.settle(4000);
        {
            let mut s = p.st.borrow_mut();
            let party: Vec<usize> = s.actors.party.active().map(|a| a.index()).collect();
            for a in (0..0x100).filter(|a| !party.contains(a)) {
                s.actors.attrs.status[a] |= 0x10;
            }
        }
        p.key(Key::Char('b'));
        p
    }

    /// The party put beside the nearest closed door as the map's teleport puts it (0x1092fc),
    /// a turn passed so the screen shows it there; the door.
    fn beside_a_door(&mut self) -> Slot {
        let (door, (bx, by)) = closed_door(&self.st.borrow());
        {
            let mut s = self.st.borrow_mut();
            let z = u16::from(s.map.centre.z);
            crate::map::travel::teleport(&mut s, bx, by, z, 0);
            crate::objects::build_draw_lists(&mut s);
            crate::map::view::update_view(&mut s);
        }
        self.key(Key::Space);
        door
    }

    fn at_prompt(&self) -> bool {
        self.e.io().pending_events() == 0 && self.e.state().main.reading == 1
    }

    /// Ticks until the prompt is up (at most `ticks`), then a few more.
    fn settle(&mut self, ticks: usize) {
        for _ in 0..ticks {
            if self.at_prompt() {
                break;
            }
            self.e.tick(&[], STEP_MS);
        }
        for _ in 0..8 {
            self.e.tick(&[], STEP_MS);
        }
        assert!(self.at_prompt(), "the prompt");
    }

    /// A key pressed and let go, then the prompt again.
    fn key(&mut self, k: Key) {
        self.e.tick(&[InputEvent::Key(k)], STEP_MS);
        self.e.tick(&[InputEvent::Release(k)], STEP_MS);
        self.settle(4000);
    }

    /// The pointer's `buttons` pressed at (x, y), then let go, a tick each.
    fn click(&mut self, (x, y): (i16, i16), buttons: u8) {
        self.e
            .tick(&[InputEvent::Pointer { x, y, buttons }], STEP_MS);
        self.e
            .tick(&[InputEvent::Pointer { x, y, buttons: 0 }], STEP_MS);
    }

    /// The frame pixel at the middle of the view cell of map tile (x, y).
    fn pixel(&self, x: u16, y: u16) -> (i16, i16) {
        let c = self.e.state().map.centre;
        let col = x as i16 - c.x as i16 + crate::geom::HALF_W as i16;
        let row = y as i16 - c.y as i16 + crate::geom::HALF_H as i16;
        (
            crate::geom::MAP_X as i16 + col * 16 + 8,
            crate::geom::MAP_Y as i16 + row * 16 + 8,
        )
    }

    /// The frame and the HD compositor's frame (4×; its font drawn by the platform, so here the
    /// glyphs scaled) into `U6_FRAMES_DIR` as `<name>-<build>.png` and `<name>-<build>-x4.png`;
    /// nothing without it. Panics on a directory inside the repository.
    fn frames(&self, name: &str) {
        let Some(dir) = std::env::var_os("U6_FRAMES_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        // The worktree, and the main checkout a linked worktree lies in (`<main>/.worktrees/x`).
        let repo =
            std::fs::canonicalize(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
                .quiet();
        let main = repo
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == ".worktrees"))
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf);
        let real = std::fs::canonicalize(&dir).unwrap_or(dir.clone());
        for root in std::iter::once(repo).chain(main) {
            assert!(
                !real.starts_with(&root),
                "the frames directory outside the repository"
            );
        }
        std::fs::create_dir_all(&dir).quiet();
        std::fs::write(dir.join(format!("{name}-{BUILD}.png")), png(self.e.frame())).quiet();
        let mut hd = HdView::new();
        hd.set(true, Scale::X4);
        hd.present(Some(&self.e), &Framebuffer::new());
        let (w, h) = (u6_render::WIDTH * 4, u6_render::HEIGHT * 4);
        let bytes: Vec<u8> = hd.rgba().iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(bytes.len(), w * h * 4, "the HD frame's size");
        std::fs::write(
            dir.join(format!("{name}-{BUILD}-x4.png")),
            png_rgba(w, h, &bytes),
        )
        .quiet();
        println!("frames: {name} ({BUILD}, and the HD compositor's 4x)");
    }
}

/// Spec 17 Acceptance 8 on the drawn menu `v` of `p`: inside its text area only the paper, the
/// ink and the heading's colour; its box differs from the published frame; outside it the screen
/// frame is the published frame. Returns the text area's pixel count.
fn drawn_in_the_frame(p: &Play, v: &View) -> usize {
    let c = p.e.state().ui.tables.colours;
    let (screen, published) = (p.e.screen_frame(), p.e.published_frame());
    let (x0, y0, x1, y1) = v.rect;
    let (tx0, ty0, tx1, ty1) = v.text;
    let (mut text, mut differ, mut boxed) = (0, 0, 0);
    for y in 0..u6_render::HEIGHT {
        for x in 0..u6_render::WIDTH {
            let (xi, yi) = (x as i32, y as i32);
            let (a, b) = (screen.get(x, y), published.get(x, y));
            if (tx0..=tx1).contains(&xi) && (ty0..=ty1).contains(&yi) {
                text += 1;
                let colour = a.quiet();
                assert!(
                    [c.paper, c.text, c.alert].contains(&colour),
                    "the text area at ({x}, {y}): paper, ink or the heading's colour"
                );
            }
            if (x0..=x1).contains(&xi) && (y0..=y1).contains(&yi) {
                boxed += 1;
                differ += usize::from(a != b);
            } else {
                assert_eq!(a, b, "outside the box at ({x}, {y})");
            }
        }
    }
    assert!(
        differ * 2 > boxed,
        "the menu drawn: {differ} of {boxed} pixels"
    );
    text
}

/// The row of entry `code`, and its pixel (the label's middle).
fn row_of(v: &View, code: u16) -> Option<(usize, (i16, i16))> {
    let i = v.rows.iter().position(|r| r.code == Some(code))?;
    Some((i, ((v.text.0 + 12) as i16, (v.rows[i].y + 4) as i16)))
}

/// Spec 17 Acceptances 1 and 8: a right press on a closed door beside the leader opens the
/// menu in the frame with the door's section (a heading), Look and Use able to run; the game
/// took no button; Use chosen by a left press on its row opens the door (its frame loses bit 2)
/// through the target read, and the prompt comes back with the menu closed.
#[test]
fn a_right_press_on_a_door_draws_the_menu_and_use_opens_the_door() {
    let Some(files) = files() else { return };
    let mut p = Play::boot(&files);
    let door = p.beside_a_door();
    let (dx, dy) = {
        let s = p.e.state();
        (s.objects.x(door), s.objects.y(door))
    };
    let at = p.pixel(dx, dy);
    let closed = p.e.state().objects.tf(door) >> 10;
    p.click(at, POINTER_RIGHT);
    assert_eq!(
        p.e.io().handler_pointer().buttons,
        0,
        "the game took no button"
    );
    let v = p.e.context_menu().expect("the menu open");
    let heads = v.rows.iter().filter(|r| r.kind == RowKind::Head).count();
    let (use_row, use_at) = row_of(&v, menu::USE).expect("Use offered");
    let (look_row, _) = row_of(&v, menu::LOOK).expect("Look offered");
    assert!(heads >= 1, "the door's section is headed");
    assert!(
        v.rows[use_row].enabled() && v.rows[look_row].enabled(),
        "Use and Look can run"
    );
    let text = drawn_in_the_frame(&p, &v);
    println!(
        "door menu: {} rows, {heads} heading(s), text area {text} pixels, box {:?}",
        v.rows.len(),
        v.rect
    );
    p.frames("menu-door");
    p.click(use_at, POINTER_LEFT);
    assert!(p.e.context_menu().is_none(), "chosen: closed");
    p.settle(4000);
    let now = p.e.state().objects.tf(door) >> 10;
    assert_eq!(now, closed ^ 4, "the door opened");
    assert!(
        p.e.screen_frame().pixels() == p.e.published_frame().pixels(),
        "no menu left"
    );
}

/// Spec 17 Acceptance 9: a right press on bare ground (a view cell whose menu is the tile's
/// Look alone, two or more cells from the leader) opens the menu with Look and Go here last, Go
/// here able to run (unit M4's stub); chosen by a left press on its row it calls `travel_to` with
/// the cell and nothing else: the leader stays, the prompt stays up, the menu closed.
#[test]
fn a_right_press_on_bare_ground_offers_go_here_and_it_calls_travel_to() {
    let Some(files) = files() else { return };
    let mut p = Play::boot(&files);
    p.beside_a_door();
    let leader = p.e.state().leader_position().expect("a leader");
    let (lx, ly) = (leader.x as i16, leader.y as i16);
    let ground = (-4..=4i16)
        .flat_map(|dy| (-4..=4i16).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| dx.abs().max(dy.abs()) >= 2)
        .map(|(dx, dy)| (lx + dx, ly + dy))
        .find_map(|(x, y)| {
            let at = p.pixel(x as u16, y as u16);
            let m = menu::query(&p.e.state(), p.e.io(), at.0, at.1)?;
            let bare = m.sections.len() == 1 && m.sections[0].object == -1;
            (bare && m.go_here.is_some() && m.cell == Some((x, y))).then_some(((x, y), at))
        });
    let (cell, at) = ground.expect("bare ground near the leader");
    p.click(at, POINTER_RIGHT);
    let v = p.e.context_menu().expect("the menu open");
    // The underlined command, when Rest or Combat, comes first (`b` above underlines Combat).
    let codes: Vec<u16> = v.rows.iter().filter_map(|r| r.code).collect();
    let rest: Vec<u16> = codes
        .iter()
        .copied()
        .filter(|&c| c != menu::REST && c != menu::COMBAT)
        .collect();
    assert_eq!(
        rest,
        [menu::LOOK, menu::GO_HERE],
        "the tile's Look, then Go here"
    );
    let (go, go_at) = row_of(&v, menu::GO_HERE).expect("Go here");
    assert!(
        v.rows[go].enabled(),
        "Go here can run (the stub: every cell)"
    );
    let text = drawn_in_the_frame(&p, &v);
    println!(
        "ground menu: {} rows, text area {text} pixels",
        v.rows.len()
    );
    p.frames("menu-ground");
    p.click(go_at, POINTER_LEFT);
    for _ in 0..20 {
        p.e.tick(&[], STEP_MS);
    }
    assert_eq!(
        p.e.travel().asked,
        [cell],
        "travel_to with the pressed cell"
    );
    assert!(
        p.e.context_menu().is_none() && p.at_prompt(),
        "closed, the prompt up"
    );
    let after = p.e.state().leader_position().expect("a leader");
    assert_eq!(
        (after.x as i16, after.y as i16),
        (lx, ly),
        "the leader stays (the stub)"
    );
}

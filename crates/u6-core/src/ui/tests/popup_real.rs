//! Real data (spec 17 Presentation, unit M3): the whole game booted from the user's zip with the
//! context menu on, the leader put beside a closed door: a right press on the door and one on
//! bare ground open the menu in the frame (its text area paper, ink and the heading's colour,
//! the published frame outside its box, the game's button untouched); Use chosen by a left press
//! opens the door, as the keyboard's Use does from the same start; Go here, chosen by its row,
//! starts spec 21's travel to the cell, which walks the party there; a disabled entry is drawn
//! half-toned; in Wide a menu on an inventory slot keeps every row whole. Frames of the open menu
//! (the frame, and the HD
//! compositor's at 4×) go to the directory `U6_FRAMES_DIR` names, which must resolve outside the
//! repository (they show the user's game); the tests print counts only, and `ran` or `skipped`.
//! Each skips without the zip, unless `U6_REQUIRE_ARCHIVE=1`, which fails it instead. The release
//! receipt is that run, in both builds, writing the frames:
//! `U6_REQUIRE_ARCHIVE=1 U6_FRAMES_DIR=<a directory outside the repository> scripts/cargo.sh test
//! -p u6-core --profile gate --lib [--features wide] -- popup_real --nocapture`, with
//! `U6_KO_PACK=<the user's ko.u6lp>` for the Go here receipt to install the user's Korean pack
//! (else a pack of no runs). The data is read only.
#![allow(clippy::disallowed_methods)] // the test reads the zip and writes the PNG receipts

use std::path::{Component, Path, PathBuf};

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

/// The user's zip for test `name`; none (the test skips, saying so) without it, unless
/// `U6_REQUIRE_ARCHIVE=1` (the release receipt), which fails the test instead.
fn archive(name: &str) -> Option<Files> {
    let files = files();
    if files.is_none() {
        assert_ne!(
            std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
            Ok("1"),
            "{name}: U6_REQUIRE_ARCHIVE=1 and the archive is missing"
        );
        println!("{name}: skipped (no archive)");
    }
    files
}

/// The directory `U6_FRAMES_DIR` names, made and resolved (links followed), or none without it.
/// Fails closed: on a relative path or one with `.` or `..`, on a part of it that exists but does
/// not resolve (a dangling link), and on a directory inside the worktree or the main checkout a
/// linked worktree lies in (`<main>/.worktrees/x`), checked on the resolved path before anything
/// is made and again on the directory made.
fn frames_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("U6_FRAMES_DIR")?);
    assert!(
        dir.is_absolute()
            && dir.components().all(|c| matches!(
                c,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )),
        "U6_FRAMES_DIR an absolute path without . or .."
    );
    let repo = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("the worktree resolves");
    let main = repo
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == ".worktrees"))
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let outside = |real: &Path| {
        for root in std::iter::once(&repo).chain(main.as_ref()) {
            assert!(
                !real.starts_with(root),
                "the frames directory outside the repository"
            );
        }
    };
    // The part of it that exists (a link counts, dangling or not), resolved; the rest after it.
    let (mut base, mut rest) = (dir.clone(), Vec::new());
    while base.symlink_metadata().is_err() {
        rest.push(base.file_name().expect("a part of the path").to_owned());
        assert!(base.pop(), "a part of U6_FRAMES_DIR exists");
    }
    let mut real = std::fs::canonicalize(&base).expect("the frames directory's base resolves");
    real.extend(rest.iter().rev());
    outside(&real);
    std::fs::create_dir_all(&real).quiet();
    let made = std::fs::canonicalize(&real).expect("the frames directory resolves");
    outside(&made);
    Some(made)
}

/// The names of `a`'s parts that differ from `b`'s, the UI's and combat's by their own parts
/// (none printed: they hold the game's text).
fn differing(a: &State, b: &State) -> Vec<&'static str> {
    let mut out = Vec::new();
    macro_rules! parts {
        ($p:ident . $q:ident : $($g:ident),*) => {
            $(if a.$p.$q.$g != b.$p.$q.$g {
                out.push(concat!(stringify!($p), ".", stringify!($q), ".", stringify!($g)));
            })*
        };
        ($p:ident : $($g:ident),*) => {
            $(if a.$p.$g != b.$p.$g {
                out.push(concat!(stringify!($p), ".", stringify!($g)));
            })*
        };
    }
    macro_rules! top {
        ($($g:ident),*) => {
            $(if a.$g != b.$g {
                out.push(stringify!($g));
            })*
        };
    }
    top!(
        tables, objects, actors, map, converse, gfx, main, magic, inventory, sound, rng, objcmds,
        saveload, speech, locale
    );
    parts!(ui: messages, font, hangul, paper, tables, text, panel);
    parts!(
        ui.input: qualifiers, accept, cursor_glyph, cursor_frame, drain, line_input, upcase,
        line_cancelled, unpage, repeat, direction, targeting, target_command, target_pending,
        cursor_by_tab, cursor, range, panel_cursor, pointer, selection, target, target_void,
        idle_refresh, in_wait, prompt_reads
    );
    parts!(
        ui.pointer: tables, shown, shape, at, left, right, pressed, buttons, press_code,
        walk_cell, walk_due
    );
    parts!(
        combat: tables, weapons, weapon_count, weapon_index, hit, landing, spread, passed, flash,
        shooter, dirty, effects, swallow, shown, said, due_ms, paced_at_ms, paced, item_cast
    );
    out
}

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
    fn click(&mut self, at: (i16, i16), buttons: u8) {
        self.press(at, buttons);
        self.release(at);
    }

    /// The pointer's `buttons` pressed at (x, y), a tick.
    fn press(&mut self, (x, y): (i16, i16), buttons: u8) {
        self.e
            .tick(&[InputEvent::Pointer { x, y, buttons }], STEP_MS);
    }

    /// The pointer's buttons let go at (x, y), a tick.
    fn release(&mut self, (x, y): (i16, i16)) {
        self.e
            .tick(&[InputEvent::Pointer { x, y, buttons: 0 }], STEP_MS);
    }

    /// A right press at (x, y) that opens the menu: right after its tick and after its release's
    /// the game has no button and its pointer events none of the press; the menu's view.
    fn open_at(&mut self, at: (i16, i16)) -> View {
        let presses = self.e.game_presses();
        self.press(at, POINTER_RIGHT);
        assert_eq!(
            self.e.io().handler_pointer().buttons,
            0,
            "the game took no button at the press"
        );
        self.release(at);
        assert_eq!(
            self.e.io().handler_pointer().buttons,
            0,
            "nor at its release"
        );
        assert_eq!(self.e.game_presses(), presses, "no press reached the game");
        self.e.context_menu().expect("the menu open")
    }

    /// The game's clock (ms).
    fn ms(&self) -> u64 {
        self.e.io().now_ms()
    }

    /// Ticks until the clock reads `ms` (or past it).
    fn run_to(&mut self, ms: u64) {
        while self.ms() < ms {
            self.e.tick(&[], STEP_MS);
        }
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
    /// nothing without it ([`frames_dir`]: fails closed on a directory that does not resolve
    /// outside the repository).
    fn frames(&self, name: &str) {
        let Some(dir) = frames_dir() else {
            return;
        };
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
/// took no button, right after the press's tick and after its release's; Use chosen by a left
/// press on its row opens the door (its frame loses bit 2) through the target read, and the
/// prompt comes back with the menu closed. The control run (round 2): from the same start the
/// keyboard's Use (its letter on the tick the menu's left press chose Use, then a left press on
/// the door once the game reads its target) opens it too, and with both runs ticked on to the
/// same clock the game's state is the same but for the keyboard's own target cursor.
#[test]
fn a_right_press_on_a_door_draws_the_menu_and_use_opens_the_door() {
    const NAME: &str = "a_right_press_on_a_door_draws_the_menu_and_use_opens_the_door";
    let Some(files) = archive(NAME) else { return };
    let mut p = Play::boot(&files);
    let door = p.beside_a_door();
    let (dx, dy) = {
        let s = p.e.state();
        (s.objects.x(door), s.objects.y(door))
    };
    let at = p.pixel(dx, dy);
    let closed = p.e.state().objects.tf(door) >> 10;
    let v = p.open_at(at);
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

    // The keyboard's Use from the same start.
    let mut k = Play::boot(&files);
    assert_eq!(k.beside_a_door(), door, "the same door");
    // The letter on the tick the menu's left press chose Use (after the right press's two).
    k.e.tick(&[], STEP_MS);
    k.e.tick(&[], STEP_MS);
    k.e.tick(&[InputEvent::Key(Key::Char('u'))], STEP_MS);
    k.e.tick(&[InputEvent::Release(Key::Char('u'))], STEP_MS);
    let mut read = false;
    for _ in 0..400 {
        let s = k.e.state();
        if s.ui.input.in_wait && s.main.reading == 0 {
            read = true;
            break;
        }
        drop(s);
        k.e.tick(&[], STEP_MS);
    }
    assert!(read, "Use reads its target");
    k.click(at, POINTER_LEFT);
    k.settle(4000);
    assert_eq!(
        k.e.state().objects.tf(door) >> 10,
        closed ^ 4,
        "the keyboard's Use opened it too"
    );
    let end = p.ms().max(k.ms());
    p.run_to(end);
    k.run_to(end);
    assert_eq!(p.ms(), k.ms(), "the same clock");
    let differ = differing(&p.st.borrow(), &k.st.borrow());
    println!(
        "control run: clock {} ms; parts of the state that differ: {}",
        p.ms(),
        differ.len()
    );
    // The keyboard's target read puts its own cursor on the map (its cell and range); a command
    // started by the pointer, as the menu's, reads the pointer and leaves that cursor as it was.
    assert_eq!(
        differ,
        ["ui.input.cursor", "ui.input.range"],
        "the menu's Use and the keyboard's leave the same state (the RNG, the world, the turn's \
         pacing), but for the keyboard's target cursor"
    );
    // The whole state compared (round 2b, opus NIT 7), the allowed parts made the same first:
    // `differing` names the parts only for the message, so a part it does not list cannot
    // differ unseen.
    let (mut menus, keys) = (p.st.borrow().clone(), k.st.borrow().clone());
    menus.ui.input.cursor = keys.ui.input.cursor;
    menus.ui.input.range = keys.ui.input.range;
    assert!(
        menus == keys,
        "the whole state the same but for the allowed parts: {:?}",
        differing(&menus, &keys)
    );
    println!("{NAME}: ran");
}

/// A view cell of bare ground two or more cells from the leader (its menu the tile's Look alone,
/// with Go here) and its pixel.
fn bare_ground(p: &Play) -> ((i16, i16), (i16, i16)) {
    let leader = p.e.state().leader_position().expect("a leader");
    let (lx, ly) = (leader.x as i16, leader.y as i16);
    (-4..=4i16)
        .flat_map(|dy| (-4..=4i16).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| dx.abs().max(dy.abs()) >= 2)
        .map(|(dx, dy)| (lx + dx, ly + dy))
        .find_map(|(x, y)| {
            let at = p.pixel(x as u16, y as u16);
            let m = menu::query(&p.e.state(), p.e.io(), at.0, at.1)?;
            let bare = m.sections.len() == 1 && m.sections[0].object == -1;
            let go = m.go_here.is_some_and(|e| e.enabled());
            (bare && go && m.cell == Some((x, y))).then_some(((x, y), at))
        })
        .expect("bare ground near the leader")
}

/// Escape closes the open menu, then the prompt again.
fn close(p: &mut Play) {
    assert!(p.e.context_menu().is_some(), "open");
    p.key(Key::Escape);
    assert!(p.e.context_menu().is_none(), "closed");
}

/// Spec 17 Input (round 2b, gpt b MUST; K2 merge notes 2, 3 and 6), on the user's game: Go here
/// chosen by a left press, its release and a right press at the same pixel in the next tick
/// (within the double-click window) open a menu, which is new input, and the press reaches no
/// game (a right press is no double click of a left one); the travel Go here started ends there,
/// before its first step (spec 21's merge contract: the menu's new input is a stop, the release
/// before it is not); a right press in the same tick after a platform's repeat of a direction key,
/// or with one left in the game's queue (the prompt takes both back), opens the menu too; after
/// a paced press (a press) and with the game's cross-hair up (no command prompt) it opens none.
#[test]
fn a_right_press_after_a_choice_or_a_repeat_taken_back_opens_the_menu() {
    const NAME: &str = "a_right_press_after_a_choice_or_a_repeat_taken_back_opens_the_menu";
    let Some(files) = archive(NAME) else { return };
    let mut p = Play::boot(&files);
    p.beside_a_door();
    let (_, at) = bare_ground(&p);
    let v = p.open_at(at);
    let (_, go_at) = row_of(&v, menu::GO_HERE).expect("Go here");
    let t0 = p.ms();
    p.press(go_at, POINTER_LEFT);
    assert_eq!(p.e.go_here_asked().asked.len(), 1, "Go here chosen");
    assert!(p.e.travel_active(), "its travel started");
    let presses = p.e.game_presses();
    let release = InputEvent::Pointer {
        x: go_at.0,
        y: go_at.1,
        buttons: 0,
    };
    let right_here = InputEvent::Pointer {
        x: go_at.0,
        y: go_at.1,
        buttons: POINTER_RIGHT,
    };
    p.e.tick(&[release, right_here], STEP_MS);
    println!("the right press {} ms after the choosing one", p.ms() - t0);
    assert_eq!(
        p.e.travel_end(),
        Some(crate::actors::travel::End::Input),
        "the menu's new input ends the travel"
    );
    assert_eq!(p.e.travel_log().len(), 0, "before its first step");
    assert!(p.ms() - t0 <= 500, "within the double-click window");
    assert_eq!(
        p.e.game_presses(),
        presses,
        "the right press at the choosing pixel reached no game"
    );
    assert!(
        p.e.context_menu().is_some(),
        "the right press at the choosing pixel opens a menu"
    );
    assert!(p.e.popup_mut().took_new_input(), "and is new input");
    p.release(go_at);
    close(&mut p);
    let rep = InputEvent::Repeat(Key::Down);
    let right = InputEvent::Pointer {
        x: at.0,
        y: at.1,
        buttons: POINTER_RIGHT,
    };
    p.e.tick(&[rep, right], STEP_MS);
    assert!(
        p.e.context_menu().is_some(),
        "after a repeat in the same tick: open"
    );
    p.release(at);
    close(&mut p);
    p.e.io().queue_event(rep);
    p.e.tick(&[right], STEP_MS);
    assert!(
        p.e.context_menu().is_some(),
        "with a repeat left in the game's queue: open"
    );
    assert_eq!(p.e.io().pending_events(), 0, "the prompt took it back");
    p.release(at);
    close(&mut p);
    p.st.borrow_mut().ui.input.targeting = 1;
    p.press(at, POINTER_RIGHT);
    assert!(
        p.e.context_menu().is_none(),
        "no menu with the cross-hair up (no command prompt)"
    );
    p.release(at);
    p.st.borrow_mut().ui.input.targeting = 0;
    let paced = InputEvent::Paced(Key::Down);
    p.e.tick(&[paced, right], STEP_MS);
    assert!(p.e.context_menu().is_none(), "after a paced press: none");
    p.release(at);
    p.settle(4000);
    println!("{NAME}: ran");
}

/// Spec 21's merge contract (round 5, opus SHOULD 1): Go here chosen by a left press, its
/// release and a right press on the map, all in one tick's input: the right press opens a menu
/// after the choice, and no travel starts under it (the engine starts Go here's travel only when
/// no menu is open after the input); the cell is recorded as asked all the same. Over quiet ticks
/// the menu stays open and the party where it stood; Escape closes it and no travel walks. Frames:
/// `r5-menu-no-walk` (the menu open after those ticks).
#[test]
fn a_menu_opened_in_the_input_that_chose_go_here_starts_no_travel() {
    const NAME: &str = "a_menu_opened_in_the_input_that_chose_go_here_starts_no_travel";
    let Some(files) = archive(NAME) else { return };
    let mut p = Play::boot(&files);
    p.beside_a_door();
    let (cell, at) = bare_ground(&p);
    let v = p.open_at(at);
    let (_, go_at) = row_of(&v, menu::GO_HERE).expect("Go here");
    let stood = p.e.state().leader_position().expect("a leader");
    let ptr = |(x, y): (i16, i16), buttons: u8| InputEvent::Pointer { x, y, buttons };
    p.e.tick(
        &[
            ptr(go_at, POINTER_LEFT),
            ptr(go_at, 0),
            ptr(at, POINTER_RIGHT),
        ],
        STEP_MS,
    );
    assert!(
        p.e.context_menu().is_some(),
        "the right press opened a menu"
    );
    assert!(!p.e.travel_active(), "no travel under it");
    assert_eq!(p.e.go_here_asked().asked, [cell], "Go here asked");
    p.release(at);
    let quiet = 30;
    for _ in 0..quiet {
        p.e.tick(&[], STEP_MS);
    }
    let now = p.e.state().leader_position().expect("a leader");
    assert!(
        p.e.context_menu().is_some() && !p.e.travel_active() && now == stood,
        "over {quiet} quiet ticks: the menu open, no travel, the party where it stood"
    );
    p.frames("r5-menu-no-walk");
    p.e.tick(&[InputEvent::Key(Key::Escape)], STEP_MS);
    p.e.tick(&[InputEvent::Release(Key::Escape)], STEP_MS);
    assert!(
        p.e.context_menu().is_none(),
        "the fixture: Escape closed it"
    );
    for _ in 0..quiet {
        p.e.tick(&[], STEP_MS);
    }
    let after = p.e.state().leader_position().expect("a leader");
    println!(
        "the menu open over {quiet} quiet ticks, then closed and {quiet} more: travel steps {}, \
         Go here asked {}, the leader moved {}",
        p.e.travel_log().len(),
        p.e.go_here_asked().asked.len(),
        u8::from(after != stood)
    );
    assert!(
        !p.e.travel_active() && p.e.travel_log().is_empty() && after == stood,
        "closed: no travel walks, the party where it stood"
    );
    println!("{NAME}: ran");
}

/// Spec 17 Acceptance 9 with spec 21 (the merge round): a right press on bare ground (a view
/// cell whose menu is the tile's Look alone, two or more cells from the leader, Go here able to
/// run: the travel's own `reachable`) opens the menu with Look and Go here last; chosen by a left
/// press on its row it starts the real travel to the cell (`Engine::travel_to`): the menu closed,
/// the party walks there, a key a step, and the travel ends on it (`Arrived`), the prompt up.
/// The menu in the port's Korean words ("여기로 이동"), the Korean pack installed as the page
/// installs it (`Engine::install_locale`, `Game::set_locale`; round 5, MUST 4: rounds 1 to 4 set
/// the text layer's language by hand, which no pack does), with the site's Hangul table, the text
/// layer's language left at its default: the user's own pack when `U6_KO_PACK` names its file
/// (outside the repository, which holds none), else a pack of no runs (the menu's words are the
/// port's own, so it shows them the same). Frames: `r4-menu-ground` (the menu over bare ground),
/// `r4-go-here-walking` (the party on its way, at the first prompt after its first step).
#[test]
fn a_right_press_on_bare_ground_offers_go_here_and_it_calls_travel_to() {
    const NAME: &str = "a_right_press_on_bare_ground_offers_go_here_and_it_calls_travel_to";
    let Some(files) = archive(NAME) else { return };
    let mut p = Play::boot(&files);
    p.beside_a_door();
    let table = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../site/fonts/galmuri7-hangul.bin"
    ));
    let hangul = u6_formats::font::HangulFont::parse(table).expect("the site's Hangul table");
    let (pack, whose) = match std::env::var_os("U6_KO_PACK") {
        Some(path) => {
            let bytes = std::fs::read(&path).expect("the pack U6_KO_PACK names");
            let pack = crate::locale::Pack::parse(&bytes).expect("a pack");
            (pack, "the user's")
        }
        None => {
            let base = crate::locale::fingerprint([]);
            let pack = crate::locale::Pack::new(crate::ui::Language::Korean, "test/1", base, [])
                .expect("a pack of no runs");
            (pack, "one of no runs")
        }
    };
    assert_eq!(pack.language, crate::ui::Language::Korean, "a Korean pack");
    p.e.install_locale(Rc::new(pack), Some(Rc::new(hangul)))
        .expect("the pack checks against the user's files");
    assert_eq!(
        p.e.state().ui.text.language,
        crate::ui::Language::English,
        "the fixture: the text layer's language as the pack leaves it"
    );
    println!("the Korean pack installed: {whose}");
    let leader = p.e.state().leader_position().expect("a leader");
    let (lx, ly) = (leader.x as i16, leader.y as i16);
    let (cell, at) = bare_ground(&p);
    let v = p.open_at(at);
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
        "Go here can run (the travel's reachable)"
    );
    let text = drawn_in_the_frame(&p, &v);
    // The cells of the text area drawn from the Hangul table: their glyph sources name a syllable.
    let syllables = {
        let f = p.e.screen_frame();
        let (tx0, ty0, tx1, ty1) = v.text;
        let hangul = |x: i32, y: i32| {
            f.src_at(x as usize, y as usize).is_some_and(|s| {
                s.kind == u6_render::Kind::Glyph && (0xac00..=0xd7a3).contains(&s.id)
            })
        };
        (ty0..=ty1)
            .step_by(8)
            .flat_map(|y| (tx0..=tx1).step_by(8).map(move |x| (x, y)))
            .filter(|&(x, y)| (0..64).any(|k| hangul(x + k % 8, y + k / 8)))
            .count()
    };
    println!(
        "ground menu: {} rows, text area {text} pixels, {syllables} cells of Hangul syllables",
        v.rows.len()
    );
    assert!(
        v.korean && syllables > 0,
        "the menu in the Korean words, drawn from the Hangul table"
    );
    p.frames("r4-menu-ground");
    let steps = (lx - cell.0).abs().max((ly - cell.1).abs());
    p.click(go_at, POINTER_LEFT);
    assert_eq!(
        p.e.go_here_asked().asked,
        [cell],
        "travel_to with the pressed cell"
    );
    assert!(p.e.context_menu().is_none(), "chosen: closed");
    assert!(p.e.travel_active(), "the travel walks");
    let (mut moved, mut ticks, mut shot) = (0, 0, false);
    let mut was = (lx, ly);
    while p.e.travel_active() && ticks < 4000 {
        p.e.tick(&[], STEP_MS);
        ticks += 1;
        let l = p.e.state().leader_position().expect("a leader");
        let now = (l.x as i16, l.y as i16);
        if now != was {
            moved += 1;
            was = now;
        }
        if moved > 0 && !shot && p.e.travel_active() && p.at_prompt() {
            p.frames("r4-go-here-walking");
            shot = true;
        }
    }
    let end = p.e.travel_end();
    println!(
        "go here: {steps} cells away (Chebyshev), the leader moved {moved} times in {ticks} ticks, \
         the travel's end {:?}",
        end.map(crate::actors::travel::End::code)
    );
    assert_eq!(
        end,
        Some(crate::actors::travel::End::Arrived),
        "the travel arrived"
    );
    assert_eq!(was, cell, "the party on the pressed cell");
    assert!(
        moved >= 1 && moved as i16 >= steps,
        "it walked a step a move"
    );
    assert!(shot, "a prompt on the way: the walking frame");
    p.settle(4000);
    assert!(
        p.e.context_menu().is_none() && p.at_prompt(),
        "closed, the prompt up"
    );
    // The opening is gated on the game accepting input (round 2, opus NIT): with that off at the
    // prompt the same press opens nothing (it is the game's).
    p.st.borrow_mut().ui.input.accept = false;
    p.press(at, POINTER_RIGHT);
    assert!(
        p.e.context_menu().is_none(),
        "no menu while the game accepts no input"
    );
    println!("{NAME}: ran");
}

/// Spec 17 Presentation (round 2, item 2): a disabled entry (the leader starts without a readied
/// spellbook, so Cast is disabled on a cell near him) is drawn half-toned in the frame: its row's
/// label cells hold ink, never two ink pixels side by side (the original's checker), the rest the
/// paper; an entry that can run holds ink side by side. Escape closes the menu and the frame is
/// the published one again. Frames: `r2-disabled`.
#[test]
fn a_disabled_entry_is_drawn_half_toned_in_the_frame() {
    const NAME: &str = "a_disabled_entry_is_drawn_half_toned_in_the_frame";
    let Some(files) = archive(NAME) else { return };
    let mut p = Play::boot(&files);
    let leader = p.e.state().leader_position().expect("a leader");
    let (lx, ly) = (leader.x as i16, leader.y as i16);
    let at = (-4..=4i16)
        .flat_map(|dy| (-4..=4i16).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| p.pixel((lx + dx) as u16, (ly + dy) as u16))
        .find(|&(x, y)| {
            menu::query(&p.e.state(), p.e.io(), x, y).is_some_and(|m| {
                m.sections
                    .iter()
                    .any(|s| s.entries.iter().any(|e| e.reason.is_some()))
            })
        })
        .expect("a cell near the leader whose menu has a disabled entry");
    let v = p.open_at(at);
    let disabled = (1..v.rows.len())
        .find(|&i| v.rows[i].kind == RowKind::Entry && !v.rows[i].enabled() && !v.rows[i].theft)
        .expect("a disabled entry drawn, past the first row");
    let enabled = (0..v.rows.len())
        .find(|&i| v.rows[i].enabled() && !v.rows[i].theft)
        .expect("an entry that can run");
    let c = p.e.state().ui.tables.colours;
    // A row's label cells (from the text area's second column): ink pixels, and ink pixels with
    // ink right beside them; any other colour than the paper and the ink is counted too.
    let band = |row: usize| {
        let frame = p.e.screen_frame();
        let (x0, x1, y) = (v.text.0 + 8, v.text.2, v.rows[row].y);
        let (mut ink, mut pairs, mut other) = (0, 0, 0);
        for yy in y..y + 8 {
            for xx in x0..=x1 {
                let at = |x: i32| frame.get(x as usize, yy as usize).quiet();
                let colour = at(xx);
                ink += usize::from(colour == c.text);
                other += usize::from(colour != c.text && colour != c.paper);
                pairs += usize::from(xx < x1 && colour == c.text && at(xx + 1) == c.text);
            }
        }
        (ink, pairs, other)
    };
    let (ink, pairs, other) = band(disabled);
    let (on_ink, on_pairs, _) = band(enabled);
    println!(
        "disabled row: {ink} ink pixels, {pairs} side by side, {other} of other colours; an \
         entry that runs: {on_ink} ink, {on_pairs} side by side"
    );
    assert!(
        ink > 0 && pairs == 0 && other == 0,
        "the disabled label half-toned"
    );
    assert!(on_pairs > 0, "an entry that runs drawn whole");
    p.frames("r2-disabled");
    p.key(Key::Escape);
    assert!(p.e.context_menu().is_none(), "closed");
    assert!(
        p.e.screen_frame().pixels() == p.e.published_frame().pixels(),
        "no menu left"
    );
    println!("{NAME}: ran");
}

/// Spec 17 Presentation (round 2, item 11), the Wide build: a right press on an item in the
/// inventory slot's backpack list (its part too narrow for the menu's widest row) opens the menu
/// with every row whole (no row cut, the text area as wide as its rows ask), placed where it
/// fits (the shown map, when the slot's part cannot hold it). Frames: `r2-wide-inventory`.
#[cfg(feature = "wide")]
#[test]
fn a_wide_inventory_slot_menu_keeps_its_rows_whole() {
    const NAME: &str = "a_wide_inventory_slot_menu_keeps_its_rows_whole";
    let Some(files) = archive(NAME) else { return };
    let mut p = Play::boot(&files);
    let slot = crate::ui::pointer::SLOT_REGIONS.start + 2;
    let [x0, y0, x1, y1] = crate::ui::pointer::region(&p.e.state().ui.pointer.tables, slot)
        .expect("the inventory slot's region");
    let mid = x0 + (x1 - x0) / 2;
    let at = (y0..=y1)
        .step_by(4)
        .flat_map(|y| (mid..=x1).step_by(4).map(move |x| (x, y)))
        .find(|&(x, y)| {
            menu::query(&p.e.state(), p.e.io(), x, y)
                .is_some_and(|m| m.kind == menu::Kind::Panel && !m.sections.is_empty())
        })
        .expect("an item in the inventory slot's list");
    let v = p.open_at(at);
    let open = p.e.popup_mut().open().expect("open").clone();
    // The fixture needs the fallback (round 2b, gpt d SHOULD): the slot's part pressed (its list,
    // below its name row) has fewer whole cells across, inside the menu's rings, than its widest
    // row asks (`place`'s room).
    let rings = open.rings.len() as i32;
    let cell = 8;
    let (list_x0, list_x1) = (i32::from(x0) + (0xf0 - 0xa8), i32::from(x1));
    let room =
        (list_x1 - rings + 1).div_euclid(cell) - (list_x0 + rings + cell - 1).div_euclid(cell);
    println!(
        "the list part: {room} cells across, the widest row {} cells",
        open.want.0
    );
    assert!(at.0 >= list_x0 as i16, "the press in the list part");
    assert!(
        room < open.want.0,
        "the list part cannot hold the widest row"
    );
    let (sx0, sy0, sx1, sy1) = crate::geom::SHOWN;
    let in_map = v.rect.0 >= i32::from(sx0)
        && v.rect.1 >= i32::from(sy0)
        && v.rect.2 <= i32::from(sx1)
        && v.rect.3 <= i32::from(sy1);
    println!(
        "inventory menu: {} rows, text area {} x {} cells (asked {} x {}), in the shown map: {}",
        v.rows.len(),
        open.text.2,
        open.text.3,
        open.want.0,
        open.want.1,
        u8::from(in_map)
    );
    assert_eq!(
        (open.text.2, open.text.3),
        open.want,
        "no row cut, no label or reason cut short"
    );
    assert!(
        in_map,
        "placed in the shown map, the part pressed being too narrow"
    );
    assert!(
        v.rect.0 >= 0
            && v.rect.1 >= 0
            && v.rect.2 < u6_render::WIDTH as i32
            && v.rect.3 < u6_render::HEIGHT as i32,
        "inside the frame"
    );
    p.frames("r2-wide-inventory");
    p.key(Key::Escape);
    assert!(p.e.context_menu().is_none(), "closed");
    println!("{NAME}: ran");
}

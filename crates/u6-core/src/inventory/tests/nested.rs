//! The inventory panel's actions inside a more prompt. The key read 0x10790a runs Ready (0xa7),
//! Unready (0xa8) and the click map's inventory handler 0x10619c while a print waits for its more
//! key (0x107f4c). The original prints and draws synchronously, so each action's lines and
//! pictures come there, in its own order, and the lines of the interrupted print come after the
//! more key. The port queues both, so the action plays its own and leaves the interrupted queue,
//! its frames and Attack's held newline as they were.
//!
//! All text is made up for the tests.

use super::*;
use crate::main_loop::{PrintArg, queue_print};
use crate::objects::commands::{GameData, ObjCommands};

/// The chest type and the trap 0x10619c looks for in it (type 0x151, quality 0x16).
const CHEST: u16 = 0x62;
const TRAP: u16 = 0x151;
/// The image address of the poison line of 0x12ba36 (the string its poison arm at 0x12bae0
/// prints; the objects code reads it from its copy of `game`, objects/commands/world.rs).
const S_POISON: u32 = 0x0012_87de;
/// The image address of the prompt 0x10619c prints after springing a trap (0x106452).
const CLICK_PROMPT: u32 = 0x0010_4ff4;
/// The colour of the interrupted print's own frame (the whole map window).
const OUTER_INK: u8 = 7;

/// The print an action interrupts: `lines` queued lines in a window that pages after two (the
/// more prompt comes while the third, `q2`, is printed), its own frame (the map window in
/// [`OUTER_INK`]) recorded after the first `mark` of them, Attack's newline held or not, and
/// either the end of turn's shape (a flush, then the playback that shows the frames and hands
/// the newline on) or the playback alone (`playback`: the prompt inside its print of the lines
/// before the frame, which holds the rest of them).
#[derive(Clone, Copy, Debug)]
struct Shape {
    lines: u16,
    mark: u16,
    swallow: bool,
    playback: bool,
}

/// Three lines still queued at the click and the newline held.
const BOTH: Shape = Shape {
    lines: 6,
    mark: 6,
    swallow: true,
    playback: false,
};
/// The same with no newline held.
const QUEUE: Shape = Shape {
    swallow: false,
    ..BOTH
};
/// The more prompt on the last line, so none is queued at the click; the newline held.
const SWALLOW: Shape = Shape {
    lines: 3,
    mark: 3,
    ..BOTH
};
/// The playback's own print: the frame after `q3`, the newline held.
const PLAYBACK: Shape = Shape {
    mark: 4,
    playback: true,
    ..BOTH
};
/// The frame after `q0`, printed before the prompt; three lines queued at the click.
const EARLY: Shape = Shape {
    mark: 1,
    swallow: false,
    ..BOTH
};
/// The same with the prompt on the last line.
const EARLY_LAST: Shape = Shape { lines: 3, ..EARLY };

impl Shape {
    /// The interrupted frame's lines were printed before the prompt, so it is due there.
    fn due(self) -> bool {
        !self.playback && self.mark <= 2
    }
}

/// A synthetic `game` with the strings the actions print (one code hunk from the image base):
/// the Ready line, the name line, the Unready line, Ready's prompt (`=` and the name) and the
/// click's prompt (`+` and the name), each prompt after a newline as the original's are. The
/// Ready and Unready lines start with a newline too, so a held newline let into the action
/// would take it.
fn strings() -> Files {
    let mut code = vec![0u8; 0x10d00];
    for (addr, text) in [
        (ready::STR_READY, &b"\nR "[..]),
        (ready::STR_NAME_LINE, b"%s\n"),
        (ready::STR_UNREADY, b"\nU %s\n"),
        (ready::STR_PROMPT, b"\n=%s\n"),
        (CLICK_PROMPT, b"\n+%s\n"),
    ] {
        let at = (addr - crate::image::IMAGE_BASE) as usize;
        code[at..at + text.len()].copy_from_slice(text);
    }
    let game = crate::image::SyntheticGame::new()
        .loaded(false, code)
        .build();
    Files::new([("game".to_string(), game)].into())
}

/// The map window (with its border), row-major.
fn map_window(fb: &crate::Framebuffer) -> Vec<u8> {
    (0..176)
        .flat_map(|y| (0..176).map(move |x| (x, y)))
        .map(|(x, y)| fb.get(x, y).unwrap_or(0))
        .collect()
}

/// The message scroll right of the map window, `rows` high (the original's is under the status
/// panel, 0x1351d8), so the text never draws into the window the tests compare.
fn scroll_beside_the_map(s: &mut State, rows: u8) {
    let t = s.ui.tables.clone();
    s.ui.text.open(
        &t,
        crate::ui::WindowId::Messages,
        0x16,
        0x0e,
        0x26,
        0x0e + rows - 1,
    );
    s.ui.text.win_mut().flags = crate::ui::text::SCROLL | crate::ui::text::PAGED;
    s.ui.text.more_rows = 3;
}

/// The engine running the print `shape` describes, at its more prompt (after its first tick).
fn print_with_a_more_prompt(mut s: State, shape: Shape, rows: u8) -> Engine {
    scroll_beside_the_map(&mut s, rows);
    s.combat.swallow = shape.swallow;
    let mut e = Engine::with_game(s, strings(), move |st, io| async move {
        let line = |st: &RefCell<State>, i: u16| {
            queue_print(
                &mut st.borrow_mut(),
                b"q%d\n".to_vec(),
                vec![PrintArg::Word(i)],
            );
        };
        for i in 0..shape.mark {
            line(&st, i);
        }
        io.draw(|fb| {
            for y in 0..176 {
                for x in 0..176 {
                    fb.set(x, y, OUTER_INK);
                }
            }
        });
        io.present();
        let flash = crate::combat::present::FLASH_MS;
        crate::combat::present::capture_window(&mut st.borrow_mut(), flash);
        for i in shape.mark..shape.lines {
            line(&st, i);
        }
        if !shape.playback {
            crate::main_loop::flush_prints(&st, &io).await;
        }
        crate::combat::play_effects(&st, &io).await;
    });
    // The playback paces each line (PRINT_MS) before printing it.
    for _ in 0..20 {
        let s = e.state();
        if s.ui.messages.len() >= 2 && s.ui.input.cursor_glyph == 1 {
            break;
        }
        drop(s);
        e.tick(&[], 16);
    }
    assert!(!e.finished(), "the more prompt after two lines");
    assert_eq!(
        (e.state().ui.messages.len(), e.state().ui.input.cursor_glyph),
        (2, 1),
        "the more prompt after two lines"
    );
    // A frame already due is shown in the prompt (its lines are printed: the original drew it
    // before the prompt), the queue set aside while it plays.
    if shape.due() {
        for _ in 0..60 {
            if e.state().combat.shown == 1 {
                break;
            }
            e.tick(&[], 16);
        }
        e.tick(&[], 16);
    }
    let queued = if shape.playback { 0 } else { shape.lines - 3 };
    assert_eq!(
        (e.state().main.pending_print.len(), e.state().combat.shown),
        (usize::from(queued), u64::from(shape.due())),
        "(lines still queued, frames shown) at the click"
    );
    e
}

/// A click at (x, y) in the panel (press and release), then `ticks` ticks; per tick the
/// transcript's length and the frames shown so far.
fn click(e: &mut Engine, x: i16, y: i16, ticks: usize) -> Vec<(usize, u64)> {
    let ev = |b| InputEvent::Pointer { x, y, buttons: b };
    let snap = |e: &Engine| (e.state().ui.messages.len(), e.state().combat.shown);
    e.tick(&[ev(1), ev(0)], 16);
    let mut seen = vec![snap(e)];
    for _ in 0..ticks {
        e.tick(&[], 16);
        seen.push(snap(e));
    }
    seen
}

/// A more key and up to `ticks` ticks after it, until the game ends.
fn more_key(e: &mut Engine, ticks: usize) {
    e.tick(&[InputEvent::Key(Key::Space)], 16);
    for _ in 0..ticks {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
}

/// What a run of [`in_a_more_prompt`] saw.
struct Seen {
    /// The transcript when the more key comes, and at the end.
    before: Vec<String>,
    lines: Vec<String>,
    /// Frames shown when the more key comes and at the end, and frames still recorded when it
    /// comes.
    shown_before: u64,
    shown_after: u64,
    left: usize,
    /// Per tick from the click's: the transcript's length and the frames shown so far.
    ticks: Vec<(usize, u64)>,
    /// The map window as the game had drawn it when the more key came, and as shown at the end.
    live: Vec<u8>,
    end: Vec<u8>,
    /// The held newline reached the text layer at the end (its skip flag set, the combat flag
    /// clear).
    handed: bool,
}

/// The print `shape` describes waits in its more prompt; a click at (x, y) in the panel runs
/// an action there, 600 ticks pass (the actor flash's two dissolves take about 7 s), then the
/// more key and up to 1000 ticks for the rest (a flash left for later takes as long).
fn in_a_more_prompt(s: State, shape: Shape, x: i16, y: i16) -> (Seen, State) {
    let mut e = print_with_a_more_prompt(s, shape, 10);
    let ticks = click(&mut e, x, y, 600);
    assert!(!e.finished(), "still in the more prompt");
    let live = e.io().draw(|fb| map_window(fb));
    let (before, shown_before, left) = {
        let s = e.state();
        let left = s.combat.effects.len();
        (s.ui.messages.clone(), s.combat.shown, left)
    };
    more_key(&mut e, 1000);
    assert!(e.finished(), "the playback ended after the more key");
    let s = e.state().clone();
    let seen = Seen {
        before,
        lines: s.ui.messages.clone(),
        shown_before,
        shown_after: s.combat.shown,
        left,
        ticks,
        live,
        end: e.io().published(map_window),
        handed: s.ui.text.skip_newline && !s.combat.swallow,
    };
    (seen, s)
}

/// The index of the line `want` in `lines`.
fn line_at(lines: &[String], want: &str) -> usize {
    lines
        .iter()
        .position(|l| l == want)
        .unwrap_or_else(|| panic!("the line {want:?} is printed: {lines:?}"))
}

/// The index of the first line of `lines` holding `part`. An action run in a more prompt adds
/// its text to the line the prompt waits in (0x107e5a keeps one line buffer), as the original's
/// does: the interrupted line and the action's first share a row.
fn line_with(lines: &[String], part: &str) -> usize {
    lines
        .iter()
        .position(|l| l.contains(part))
        .unwrap_or_else(|| panic!("a line with {part:?} is printed: {lines:?}"))
}

/// The lines of the interrupted print that were still to come at the click (`q3` on).
fn later(l: &str) -> bool {
    ["q3", "q4", "q5"].contains(&l)
}

/// The checks every shape shares: the action's first line (holding `first`) and its prompt
/// (`prompt`) are printed before the more key, each after its own newline (the first one ends
/// the interrupted line `q2`, which the prompt waits in); none of the interrupted print's
/// lines still to come is printed
/// before the more key and all of them follow the prompt; the held newline (if any) reaches
/// the text layer only at the end; the interrupted print's own frame is shown where it is due
/// (at the click when its lines were printed before the prompt, else after the more key) and
/// nothing else is shown after the key.
fn the_outer_print_waits(seen: &Seen, shape: Shape, first: &str, prompt: &str) {
    let at = line_with(&seen.before, first);
    assert_eq!(
        seen.before[at - 1],
        "q2",
        "the action's leading newline is printed: {:?}",
        seen.before
    );
    let at = line_at(&seen.before, prompt);
    assert_eq!(
        seen.before[at - 1],
        "",
        "the prompt's leading newline is printed: {:?}",
        seen.before
    );
    assert!(
        !seen.before.iter().any(|l| later(l)),
        "the queued lines wait for the more key: {:?}",
        seen.before
    );
    let at = line_at(&seen.lines, prompt);
    // After the prompt: the end of the line the more prompt waited in (its text went out with
    // the action's first newline), then every queued line in order.
    let want = ["", "q3", "q4", "q5"];
    assert_eq!(
        seen.lines[at + 1..],
        want[..usize::from(shape.lines - 2)],
        "the interrupted print's rest after the prompt, exactly: {:?}",
        seen.lines
    );
    assert_eq!(
        seen.handed, shape.swallow,
        "the held newline is kept for the end of the interrupted print"
    );
    let waiting = !(shape.playback || shape.due());
    assert_eq!(
        seen.left,
        usize::from(waiting),
        "only the interrupted print's frame, not yet due, is left for after the more key"
    );
    assert_eq!(
        seen.shown_after - seen.shown_before,
        u64::from(!shape.due()),
        "only the interrupted print's frame is shown after the more key, unless it was due"
    );
}

/// The first tick (from the click's) on which `n` frames had been shown: the transcript's
/// length then.
fn lines_when_shown(seen: &Seen, n: u64) -> usize {
    seen.ticks
        .iter()
        .find(|&&(_, shown)| shown >= n)
        .map(|&(lines, _)| lines)
        .unwrap_or_else(|| panic!("{n} frames shown before the more key: {:?}", seen.ticks))
}

/// The first tick (from the click's) on which the transcript had `n` lines: the frames shown
/// then (what one tick prints and shows is on the screen together).
fn shown_when_printed(seen: &Seen, n: usize) -> u64 {
    seen.ticks
        .iter()
        .find(|&&(lines, _)| lines >= n)
        .map(|&(_, shown)| shown)
        .unwrap_or_else(|| panic!("{n} lines printed before the more key: {:?}", seen.ticks))
}

/// A closed chest (frame 1) with a trap inside, carried by actor 1 and first in the panel's
/// list; the poison draw next (status bit 3, the hit flash 0x1230ce, one frame); the hit flash
/// on; the poison line `trap` (made up, plain letters: `<` and `>` switch the text layer to
/// runes) in the objects' copy of `game`. Container types A4-0x670c: the chest.
fn trapped_chest(trap_line: &[u8]) -> State {
    let (mut s, _) = panel_world();
    s.objects.defs.type_set_670c = vec![CHEST];
    let chest = give(&mut s, CHEST | 1 << 10, IN_INVENTORY, 0, 1);
    let trap = give(&mut s, TRAP, IN_CONTAINER, 0, chest);
    s.objects.qq[trap as usize] = 0x1600;
    s.inventory.cells[0] = chest;
    s.objcmds = ObjCommands::from_data(GameData::with_bytes(&[(S_POISON, trap_line)]));
    s.combat.flash = 1;
    let seed = (0..=u16::MAX)
        .find(|&w| crate::Rng::seeded(w).range(0, 3) == 1)
        .expect("a seed whose first draw is poison");
    s.rng = crate::Rng::seeded(seed);
    s
}

/// The trapped chest clicked inside the more prompt: the trap's line and its frame come there,
/// the frame after the line and before the click's prompt, as 0x12ba36 prints and draws
/// before 0x10619c prints the prompt (0x106438..0x106456); the interrupted print goes on after
/// the more key.
fn a_trapped_chest(shape: Shape) {
    let s = trapped_chest(b"\ntrap\n\0");
    let (seen, s) = in_a_more_prompt(s, shape, 0xf8 + 8, 0x20 + 8);
    assert_ne!(s.actors.attrs.status[1] & 0x08, 0, "the poison trap sprang");
    let trap = line_with(&seen.before, "trap");
    let prompt = line_at(&seen.before, "+Ann");
    assert!(trap < prompt, "the trap's line before the prompt");
    let outer = u64::from(shape.due());
    assert_eq!(
        seen.shown_before,
        outer + 1,
        "the trap's frame is shown in place"
    );
    let lines = lines_when_shown(&seen, outer + 1);
    assert!(
        lines > trap,
        "the frame after the trap's line: {lines} lines (line {trap})"
    );
    assert_eq!(
        shown_when_printed(&seen, prompt + 1),
        outer + 1,
        "the frame by the time the prompt is printed"
    );
    the_outer_print_waits(&seen, shape, "trap", "+Ann");
}

#[test]
fn a_trapped_chest_in_a_more_prompt_plays_in_place() {
    a_trapped_chest(BOTH);
}

#[test]
fn a_trapped_chest_in_a_more_prompt_with_lines_queued_plays_in_place() {
    a_trapped_chest(QUEUE);
}

#[test]
fn a_trapped_chest_in_a_more_prompt_with_the_newline_held_plays_in_place() {
    a_trapped_chest(SWALLOW);
}

#[test]
fn a_trapped_chest_in_a_playbacks_more_prompt_plays_in_place() {
    a_trapped_chest(PLAYBACK);
}

/// The ring of invisibility readied (or, `off`, taken off) inside the more prompt: the actor's
/// flash (0x132338, two windows) is shown there before the prompt, as 0x112052 / 0x1123d6 draw
/// it before their prompt. Before it, the interrupted print's frame when that was due.
fn a_ring(shape: Shape, off: bool) -> Seen {
    let (mut s, ring) = panel_world();
    let (x, y) = if off {
        s.objects.obj_status[ring as usize] =
            (s.objects.obj_status[ring as usize] & !WHERE_MASK) | READIED;
        s.objects.obj_status[1] |= 2;
        s.inventory.set_equip(3, ring);
        (0xb8, 0x40)
    } else {
        s.inventory.cells[0] = ring;
        (0xf8 + 8, 0x20 + 8)
    };
    let (seen, s) = in_a_more_prompt(s, shape, x, y);
    assert_eq!(
        s.objects.obj_status[1] & 2 == 0,
        off,
        "the ring changed the actor's visibility"
    );
    let outer = u64::from(shape.due());
    assert_eq!(seen.shown_before, outer + 2, "the flash is shown at once");
    let prompt = line_at(&seen.before, "=Ann");
    let lines = lines_when_shown(&seen, outer + 1);
    assert!(
        lines <= prompt,
        "the flash's first window before the prompt: {lines} lines (prompt {prompt})"
    );
    assert_eq!(
        shown_when_printed(&seen, prompt + 1),
        outer + 2,
        "the flash by the time the prompt is printed"
    );
    let first = if off { "U " } else { "R " };
    the_outer_print_waits(&seen, shape, first, "=Ann");
    seen
}

#[test]
fn a_ring_readied_in_a_more_prompt_flashes_at_once() {
    a_ring(BOTH, false);
}

#[test]
fn a_ring_readied_in_a_more_prompt_with_lines_queued_flashes_at_once() {
    a_ring(QUEUE, false);
}

#[test]
fn a_ring_readied_in_a_more_prompt_with_the_newline_held_flashes_at_once() {
    a_ring(SWALLOW, false);
}

#[test]
fn a_ring_readied_in_a_playbacks_more_prompt_flashes_at_once() {
    a_ring(PLAYBACK, false);
}

#[test]
fn a_ring_taken_off_in_a_more_prompt_flashes_at_once() {
    a_ring(BOTH, true);
}

#[test]
fn a_ring_taken_off_in_a_more_prompt_with_lines_queued_flashes_at_once() {
    a_ring(QUEUE, true);
}

#[test]
fn a_ring_taken_off_in_a_more_prompt_with_the_newline_held_flashes_at_once() {
    a_ring(SWALLOW, true);
}

#[test]
fn a_ring_taken_off_in_a_playbacks_more_prompt_flashes_at_once() {
    a_ring(PLAYBACK, true);
}

#[test]
fn a_ring_readied_inside_a_playback_leaves_the_window_as_the_game_has_it() {
    // The playback's last picture is the window as the interrupted step left it. Readied in its
    // more prompt, the ring changes the window (the flash, the actor no longer drawn); after the
    // more key the playback shows its own frame, then ends on the window as the game has it now,
    // not on the picture it took before the ring.
    let seen = a_ring(PLAYBACK, false);
    let taken = vec![OUTER_INK; 176 * 176];
    assert_ne!(seen.live, taken, "the ring changed the window");
    assert!(
        seen.end == seen.live,
        "the window after the playback is the one the game drew last"
    );
}

#[test]
fn a_frame_whose_lines_were_printed_comes_before_the_action() {
    // The end of turn and Get print their lines with a flush and show the frames after it
    // (main_loop/commands.rs end_of_turn, objects/commands/get.rs); the original drew each frame
    // where it was made (0x1213de / 0x12a962), before the lines after it. A frame made after the
    // first line is on the screen while the more prompt of the third waits (the prompt shows
    // it), so a ring readied there flashes after it, not before it (and the frame is not shown
    // again later).
    for shape in [EARLY, EARLY_LAST] {
        let seen = a_ring(shape, false);
        let ready = line_with(&seen.before, "R ");
        assert!(
            !seen
                .ticks
                .iter()
                .any(|&(lines, shown)| lines > ready && shown == 0),
            "{shape:?}: no tick with the Ready line printed and the interrupted print's frame \
             not yet shown: {:?}",
            seen.ticks
        );
    }
}

#[test]
fn an_action_inside_an_actions_more_prompt_comes_back_first() {
    // A trapped chest clicked in the more prompt of the third queued line prints three lines and
    // its prompt in a scroll five rows high, so its prompt waits for a more key too; a ring
    // readied there flashes and prompts. Each action sets aside the print it interrupts: the
    // ring's ends first and gives the trap its own print back (nothing queued, no newline
    // held), then the trap's gives back the interrupted queue, its frame and the held newline.
    let mut s = trapped_chest(b"t1\nt2\nt3\n\0");
    let ring = give(&mut s, 0x102, IN_INVENTORY, 0, 1);
    s.inventory.cells[1] = ring;
    let mut e = print_with_a_more_prompt(s, BOTH, 5);
    let held = |e: &Engine| {
        let s = e.state();
        let c = &s.combat;
        (s.main.pending_print.len(), c.effects.len(), c.swallow)
    };
    click(&mut e, 0xf8 + 8, 0x20 + 8, 200);
    assert!(!e.finished(), "the trap's prompt waits for its more key");
    let at_ring = e.state().combat.shown;
    assert_eq!(held(&e), (0, 0, false), "the trap's own print, empty");
    click(&mut e, 0x108 + 8, 0x20 + 8, 600);
    let (lines, shown) = {
        let s = e.state();
        (s.ui.messages.clone(), s.combat.shown)
    };
    assert_eq!(
        (at_ring, shown),
        (1, 3),
        "the trap's frame, then the ring's flash, before any more key"
    );
    let ready = line_at(&lines, "=Ann");
    assert!(
        line_with(&lines, "t1") < line_at(&lines, "t3") && line_at(&lines, "t3") < ready,
        "the ring after the trap's lines: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| later(l)),
        "the queue waits: {lines:?}"
    );
    assert_eq!(
        held(&e),
        (0, 0, false),
        "the ring gave the trap's print back"
    );
    more_key(&mut e, 50);
    assert!(!e.finished(), "the first more key ends the trap's prompt");
    assert_eq!(
        held(&e),
        (3, 1, true),
        "the trap gave the interrupted print back: three lines, its frame, the newline"
    );
    assert!(
        !e.state().ui.messages.iter().any(|l| later(l)),
        "the queue waits for its own more key"
    );
    // The interrupted print's rest pages in the five rows too.
    for _ in 0..3 {
        if !e.finished() {
            more_key(&mut e, 1000);
        }
    }
    assert!(
        e.finished(),
        "the interrupted print ends after its more keys"
    );
    let s = e.state();
    let lines = &s.ui.messages;
    // After the ring's prompt: the end of the trap's prompt line (its text went out with the
    // ring's first newline), the end of the interrupted line, then every queued line in order.
    assert_eq!(
        lines[ready + 1..],
        ["", "", "q3", "q4", "q5"],
        "the interrupted print after both actions, exactly: {lines:?}"
    );
    assert_eq!(s.combat.shown, 4, "the interrupted frame after the queue");
    assert!(
        s.ui.text.skip_newline && !s.combat.swallow,
        "the held newline handed on at the end"
    );
}

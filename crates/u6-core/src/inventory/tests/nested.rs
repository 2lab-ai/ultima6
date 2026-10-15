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

/// How the print the action interrupts waits for its more key. In each a frame of its own is
/// recorded after its lines (the window in one colour), to be shown after the more key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outer {
    /// The end of turn's shape (a flush, then the playback hands the held newline on): six lines
    /// in a window that pages after two, so three are still queued at the click, and Attack's
    /// newline held.
    Both,
    /// The same with no newline held.
    Queue,
    /// Three lines: the more prompt comes on the last, so none is queued at the click; the
    /// newline is held.
    Swallow,
}

impl Outer {
    fn lines(self) -> u16 {
        if self == Outer::Swallow { 3 } else { 6 }
    }

    fn swallow(self) -> bool {
        self != Outer::Queue
    }
}

/// A synthetic `game` with the strings the actions print (one code hunk from the image base):
/// the Ready line, the name line, the Unready line, Ready's prompt (`=` and the name) and the
/// click's prompt (`+` and the name), each prompt after a newline as the original's are.
fn strings() -> Files {
    let mut code = vec![0u8; 0x10d00];
    for (addr, text) in [
        (ready::STR_READY, &b"R "[..]),
        (ready::STR_NAME_LINE, b"%s\n"),
        (ready::STR_UNREADY, b"U %s\n"),
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

/// What a run of [`in_a_more_prompt`] saw.
struct Seen {
    /// The transcript when the more key comes, and at the end.
    before: Vec<String>,
    lines: Vec<String>,
    /// Frames shown when the more key comes and at the end, and frames still recorded when it
    /// comes (the interrupted print's own is one).
    shown_before: u64,
    shown_after: u64,
    left: usize,
    /// Per tick from the click's: the transcript's length and the frames shown so far.
    ticks: Vec<(usize, u64)>,
    /// When the more key comes, the screen shows the map window as it was last drawn.
    current: bool,
    /// The held newline reached the text layer at the end (its skip flag set, the combat flag
    /// clear).
    handed: bool,
}

/// The print `outer` describes waits in its more prompt; a click at (x, y) in the panel runs
/// an action there, 600 ticks pass (the actor flash's two dissolves take about 7 s), then the
/// more key and up to 1000 ticks for the rest (a flash left for later takes as long).
fn in_a_more_prompt(mut s: State, outer: Outer, x: i16, y: i16) -> (Seen, State) {
    s.ui.text.win_mut().flags |= crate::ui::text::PAGED;
    s.ui.text.more_rows = 3;
    for i in 0..outer.lines() {
        queue_print(&mut s, b"q%d\n".to_vec(), vec![PrintArg::Word(i)]);
    }
    s.combat.swallow = outer.swallow();
    let mut e = Engine::with_game(s, strings(), |st, io| async move {
        io.draw(|fb| {
            for y in 0..176 {
                for x in 0..176 {
                    fb.set(x, y, 7);
                }
            }
        });
        io.present();
        let flash = crate::combat::present::FLASH_MS;
        crate::combat::present::capture_window(&mut st.borrow_mut(), flash);
        crate::main_loop::flush_prints(&st, &io).await;
        crate::combat::play_effects(&st, &io).await;
    });
    e.tick(&[], 16);
    assert!(!e.finished(), "the more prompt after two lines");
    let queued = if outer == Outer::Swallow { 0 } else { 3 };
    assert_eq!(
        e.state().main.pending_print.len(),
        queued,
        "lines still queued at the click"
    );
    let ev = |b| InputEvent::Pointer { x, y, buttons: b };
    let snap = |e: &Engine| (e.state().ui.messages.len(), e.state().combat.shown);
    e.tick(&[ev(1), ev(0)], 16);
    let mut ticks = vec![snap(&e)];
    for _ in 0..600 {
        e.tick(&[], 16);
        ticks.push(snap(&e));
    }
    assert!(!e.finished(), "still in the more prompt");
    let current = e.io().published(map_window) == e.io().draw(|fb| map_window(fb));
    let (before, shown_before, left) = {
        let s = e.state();
        (
            s.ui.messages.clone(),
            s.combat.shown,
            s.combat.effects.len(),
        )
    };
    e.tick(&[InputEvent::Key(Key::Space)], 16);
    for _ in 0..1000 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished(), "the playback ended after the more key");
    let s = e.state().clone();
    let seen = Seen {
        before,
        lines: s.ui.messages.clone(),
        shown_before,
        shown_after: s.combat.shown,
        left,
        ticks,
        current,
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

/// The checks every variant shares: the action's prompt (`prompt`) is printed before the more
/// key after its own newline; none of the lines still queued at the click is printed before
/// the more key and all of them follow the prompt; the held newline (if any) reaches the text
/// layer only at the end; of what is recorded only the interrupted print's own frame is left for
/// after the more key, and only it is shown then.
fn the_outer_print_waits(seen: &Seen, outer: Outer, prompt: &str) {
    let at = line_at(&seen.before, prompt);
    assert_eq!(
        seen.before[at - 1],
        "",
        "the prompt's leading newline is printed: {:?}",
        seen.before
    );
    let queued = |l: &String| ["q3", "q4", "q5"].contains(&l.as_str());
    assert!(
        !seen.before.iter().any(queued),
        "the queued lines wait for the more key: {:?}",
        seen.before
    );
    let at = line_at(&seen.lines, prompt);
    let rest: Vec<usize> = (0..seen.lines.len())
        .filter(|&i| queued(&seen.lines[i]))
        .collect();
    let want = if outer == Outer::Swallow { 0 } else { 3 };
    assert_eq!(
        rest.len(),
        want,
        "every queued line is printed: {:?}",
        seen.lines
    );
    assert!(
        rest.iter().all(|&i| i > at),
        "the queued lines follow the prompt: {:?}",
        seen.lines
    );
    assert_eq!(
        seen.handed,
        outer.swallow(),
        "the held newline is kept for the end of the interrupted print"
    );
    assert_eq!(
        seen.left, 1,
        "only the interrupted print's frame is left for after the more key"
    );
    assert_eq!(
        seen.shown_after,
        seen.shown_before + 1,
        "only the interrupted print's frame is shown after the more key"
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
/// on; the poison line made up in the objects' copy of `game` (plain letters: `<` and `>`
/// switch the text layer to runes). Container types A4-0x670c: the chest.
fn trapped_chest() -> State {
    let (mut s, _) = panel_world();
    s.objects.defs.type_set_670c = vec![CHEST];
    let chest = give(&mut s, CHEST | 1 << 10, IN_INVENTORY, 0, 1);
    let trap = give(&mut s, TRAP, IN_CONTAINER, 0, chest);
    s.objects.qq[trap as usize] = 0x1600;
    s.inventory.cells[0] = chest;
    s.objcmds = ObjCommands::from_data(GameData::with_bytes(&[(S_POISON, b"trap\n\0")]));
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
fn a_trapped_chest(outer: Outer) {
    let (seen, s) = in_a_more_prompt(trapped_chest(), outer, 0xf8 + 8, 0x20 + 8);
    assert_ne!(s.actors.attrs.status[1] & 0x08, 0, "the poison trap sprang");
    let trap = line_at(
        &seen.before,
        &seen
            .before
            .iter()
            .find(|l| l.contains("trap"))
            .cloned()
            .unwrap_or_else(|| panic!("the trap's line before the more key: {:?}", seen.before)),
    );
    let prompt = line_at(&seen.before, "+Ann");
    assert!(trap < prompt, "the trap's line before the prompt");
    assert_eq!(seen.shown_before, 1, "the trap's frame is shown in place");
    let lines = lines_when_shown(&seen, 1);
    assert!(
        lines > trap,
        "the frame after the trap's line: {lines} lines (line {trap})"
    );
    assert_eq!(
        shown_when_printed(&seen, prompt + 1),
        1,
        "the frame by the time the prompt is printed"
    );
    the_outer_print_waits(&seen, outer, "+Ann");
}

#[test]
fn a_trapped_chest_in_a_more_prompt_plays_in_place() {
    a_trapped_chest(Outer::Both);
}

#[test]
fn a_trapped_chest_in_a_more_prompt_with_lines_queued_plays_in_place() {
    a_trapped_chest(Outer::Queue);
}

#[test]
fn a_trapped_chest_in_a_more_prompt_with_the_newline_held_plays_in_place() {
    a_trapped_chest(Outer::Swallow);
}

/// The ring of invisibility readied (or, `off`, taken off) inside the more prompt: the actor's
/// flash (0x132338, two windows) is shown there before the prompt, as 0x112052 / 0x1123d6 draw
/// it before their prompt, and the screen keeps the window as last drawn.
fn a_ring(outer: Outer, off: bool) {
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
    let (seen, s) = in_a_more_prompt(s, outer, x, y);
    assert_eq!(
        s.objects.obj_status[1] & 2 == 0,
        off,
        "the ring changed the actor's visibility"
    );
    assert_eq!(seen.shown_before, 2, "the flash is shown at once");
    let prompt = line_at(&seen.before, "=Ann");
    let lines = lines_when_shown(&seen, 1);
    assert!(
        lines <= prompt,
        "the flash's first window before the prompt: {lines} lines (prompt {prompt})"
    );
    assert_eq!(
        shown_when_printed(&seen, prompt + 1),
        2,
        "the flash by the time the prompt is printed"
    );
    assert!(seen.current, "the screen shows the window as last drawn");
    the_outer_print_waits(&seen, outer, "=Ann");
}

#[test]
fn a_ring_readied_in_a_more_prompt_flashes_at_once() {
    a_ring(Outer::Both, false);
}

#[test]
fn a_ring_readied_in_a_more_prompt_with_lines_queued_flashes_at_once() {
    a_ring(Outer::Queue, false);
}

#[test]
fn a_ring_readied_in_a_more_prompt_with_the_newline_held_flashes_at_once() {
    a_ring(Outer::Swallow, false);
}

#[test]
fn a_ring_taken_off_in_a_more_prompt_flashes_at_once() {
    a_ring(Outer::Both, true);
}

#[test]
fn a_ring_taken_off_in_a_more_prompt_with_lines_queued_flashes_at_once() {
    a_ring(Outer::Queue, true);
}

#[test]
fn a_ring_taken_off_in_a_more_prompt_with_the_newline_held_flashes_at_once() {
    a_ring(Outer::Swallow, true);
}

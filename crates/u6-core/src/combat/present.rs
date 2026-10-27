//! What the combat code puts on the screen: missile flights and blasts (hunk 18), and the log of
//! frames the synchronous core presented, played back by [`play_effects`].
//!
//! The flight 0x132706 draws into the back buffer and copies each step's changed rectangle to
//! the screen (0x151fe6); the hit flash 0x1230ce presents a whole window (0x102c90). The core
//! does exactly that on the polling engine's buffers ([`with_current_io`]) and records each
//! presented rectangle with the number of messages queued before it ([`capture`]). Playback
//! puts the rectangles back on the screen in order, prints the messages in between, and waits
//! so the pictures are seen.
//!
//! Timing. Nothing in a fight waits for the display in the original: its VBlank server 0x14f27a
//! (`jmp 0x162294`) only rewrites BEAMCON0, clears the flag 0x1624b0 and moves the pointer
//! sprite (0x1051a6, 0x1622d6), and the one routine that spins on that flag, 0x1622be, is reached
//! only from the shutdown 0x14ee70 and from 0x1623be, whose sole caller is the Use machine
//! 0x12cfe4. A fight is as slow as the 68000 and the blitter draw it. The port draws at once, so
//! each picture carries the time the original spends making it ([`Effect::cost`], measured in the
//! oracle), and playback waits that long on the engine clock before showing it ([`pace`]): the
//! old picture stays up while the original would still be drawing, the same at any host frame
//! rate (spec 03 §1).

use std::cell::RefCell;

use crate::engine::{Io, with_current_io};
use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, SHOWN, VIEW_H, VIEW_W};
use crate::gfx::blit::{blit_back, copy_rect};
use crate::gfx::cache::Sources;
use crate::state::State;

use super::tables::at;

/// Milliseconds of one PAL frame (the original's display runs at 50 Hz).
pub const FRAME_MS: u16 = 20;
/// [replaced] Time of the hit flash 0x1230ce (draw lists, the window with tile 0x101 rendered
/// and copied, draw lists again): 312 ms, 15.602 PAL frames, the median of 38 calls traced in
/// the oracle (15.32..16.03; the r4 and r5 runs pooled, re/notes/oracle.md; tools/oracle/trace.mjs
/// on the CPU clock, reduced by trace.py). fight.py's CPU samples give 16 (n = 45).
pub const FLASH_MS: u16 = 312;
/// [replaced] Time of a render of the window into the back buffer (0x102c90 without the frame)
/// and what a flight does before its first step: 128 ms, 6.379 PAL frames, the median of the 57
/// traced flights that drew no step (6.15..7.33; the CPU samples of 87 flights give 6).
pub const RENDER_MS: u16 = 128;
/// [replaced] Time of a render of the window with its frame, copied to the screen (0x102c90 with
/// D0 = 1): 178 ms, 8.90 PAL frames, timed at the prompt on the CPU clock (tools/oracle/timeit.mjs
/// render; 8.74 from the other idle render it can stop at).
pub const WINDOW_MS: u16 = 178;
/// A flight's first render (0x102c90 at the top of 0x132706, before the first step).
pub const FLIGHT_START_MS: u16 = RENDER_MS;
/// [replaced] Time of one drawn step of a flight (the rounds up to it, the sprites saved and
/// drawn, what lies over them, the rectangle copied, 0x151fe6): 46 ms, 2.31 PAL frames. A
/// sampled estimate: fight.py's 525 frames over 227 changes of the view in the steps of the 40
/// flights that drew any (no routine boundary to trace; the 51 whole flights traced in the start
/// fight, 6.15..35.58 frames, carry no count of their steps to check it against).
pub const FLIGHT_STEP_MS: u16 = 46;
/// [replaced] Time of a view refresh with the window shown (0x102ef0: the animation tick, the
/// window rendered by 0x102c90 and copied to the screen): 11 PAL frames. A sampled estimate:
/// the median of the 40 flights whose CPU samples end in one (9..42, 29 of them 10 or 11); with
/// the view flags before it, the traced refresh after a turn
/// ([`crate::actors::turn::AFTER_TURN_MS`], 21.336).
pub const REFRESH_MS: u16 = 11 * FRAME_MS;
/// [replaced] Time of one message (0x108658 drawing into the scroll, scrolling it): 22 ms, the
/// mean of 128 calls traced in the oracle, 1.12476 PAL frames = 22.495 ms (0.06..3.60, the ones
/// that scroll the longest), to the nearest ms as the other constants. Per call, as the port
/// prints one queued message a call.
pub const PRINT_MS: u16 = 22;
/// [replaced] Time of an object's name read for a message (0x10eef8: 0x10ee76 decodes it out of
/// the look.lzd copy in memory, 0x161ec8): 26 ms, 1.289 PAL frames, the mean of 70 calls traced
/// in the oracle (1.01..1.72). The first read of a boot also loads the file from the
/// floppy (132.23 frames traced): not modelled.
pub const NAME_MS: u16 = 26;
/// [replaced] Time of a window presented dissolving (0x102c90 with A4-0x6578 set: 0x1520e2
/// copies the 25,600 pixels of the window one by one): 3422 ms, 171.08 PAL frames, timed at the
/// prompt on the CPU clock (tools/oracle/timeit.mjs dissolve; none happens in the start fight).
pub const DISSOLVE_MS: u16 = 3422;
/// [replaced] Time of each window of the actor flash 0x132338 (the outlined actor presented
/// dissolving, then the window without it, dissolving too): half of its 345.83 PAL frames, timed
/// at the prompt on the hero (tools/oracle/timeit.mjs actor, the same on the CPU clock).
pub const ACTOR_FLASH_MS: u16 = 3458;
/// The map window with its border, in frame pixels: its top left, the map's corner (0, 0 in
/// Original), and its size, the view's cells (176 × 176 in Original).
const WINDOW_AT: (usize, usize) = (MAP_X as usize, MAP_Y as usize);
const WINDOW: (usize, usize) = (VIEW_W * 16, VIEW_H * 16);
/// The shown map ([`SHOWN`], (8, 8)..(0xa7, 0xa7) in Original) as the flight's pixel words.
const SHOWN_PX: (i16, i16, i16, i16) = (
    SHOWN.0 as i16,
    SHOWN.1 as i16,
    SHOWN.2 as i16,
    SHOWN.3 as i16,
);
/// The flight's changed rectangle A4+0x1900.. when nothing changed: from past the shown map's far
/// corner to before its near one ([0xa8, 0xa8, 7, 7] in Original, 0x132706).
pub(crate) const NO_DIRTY: [i16; 4] = [
    SHOWN_PX.2 + 1,
    SHOWN_PX.3 + 1,
    SHOWN_PX.0 - 1,
    SHOWN_PX.1 - 1,
];

/// Port: waits until the original would have spent `ms` more on what it draws, on the engine
/// clock, and at least one display frame, so what was presented before is published.
///
/// The waits of one run (nothing else waited in between) follow one schedule: each is due `ms`
/// after the previous one was due, so what a wait ends late on the host's tick the waits after
/// it make up, and the run takes the same engine time at any tick size. No wait ends more than
/// one tick early for it, the tick it ends on (judged anew at every frame of the wait), so a
/// host that stalls for one long tick (even the run's first) squeezes no wait after it by more
/// than a tick: the stall is made up a tick a wait. After anything else waited (a key, a
/// prompt) the schedule starts again from now.
pub async fn pace(state: &RefCell<State>, io: &Io, ms: u16) {
    let due = schedule(state, io, ms);
    loop {
        let (now, tick) = display_frame(io).await;
        if now >= due.at(tick) {
            break;
        }
    }
    finish(state, io, &due);
}

/// One wait on the schedule of [`pace`].
struct Due {
    /// When the wait before it in the run was due (`None`: the run starts with this wait).
    prev: Option<u64>,
    /// When this wait began.
    start: u64,
    ms: u16,
}

impl Due {
    /// When it is due on the schedule: `ms` after the wait before it, or after it began.
    fn on_schedule(&self) -> u64 {
        self.prev.unwrap_or(self.start) + u64::from(self.ms)
    }

    /// When it may end at a frame that took `tick` ms: on the schedule, but not more than that
    /// tick before its own `ms` are up.
    fn at(&self, tick: u64) -> u64 {
        let own = self.start + u64::from(self.ms);
        self.on_schedule().max(own.saturating_sub(tick))
    }
}

/// A wait of `ms` on the schedule of [`pace`], counted, not awaited.
fn schedule(state: &RefCell<State>, io: &Io, ms: u16) -> Due {
    let mut s = state.borrow_mut();
    let now = io.now_ms();
    let c = &mut s.combat;
    let prev = (now == c.paced_at_ms).then_some(c.due_ms);
    *c.paced.entry(ms).or_default() += 1;
    Due {
        prev,
        start: now,
        ms,
    }
}

/// The end of a wait: the run goes on from when it was due on the schedule (what it ended late
/// or early is the next waits' to make up).
fn finish(state: &RefCell<State>, io: &Io, due: &Due) {
    let mut s = state.borrow_mut();
    s.combat.due_ms = due.on_schedule();
    s.combat.paced_at_ms = io.now_ms();
}

/// One display frame of a paced wait: the engine time after it and the tick it took.
async fn display_frame(io: &Io) -> (u64, u64) {
    let before = io.now_ms();
    io.vblank().await;
    let now = io.now_ms();
    (now, now - before)
}

/// One presented rectangle of the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    /// Top-left pixel and size (0 × 0: no picture, only the time).
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    /// The screen's pixels there, row-major.
    pub pixels: Vec<u8>,
    /// Port-only (spec 07 HD0): their sources when the screen has a source plane, put back with
    /// them by the playback. Not game state ([`Sources`]).
    pub src: Sources,
    /// Engine milliseconds the original spends making it: [`play_effects`] waits that long
    /// before showing it.
    pub cost: u16,
    /// Messages queued (`main.pending_print`) before it was presented, the one being printed
    /// included: one less after each is printed ([`printed_one`]), 0 once they all are. That
    /// holds for frames recorded before the entry in flight left the queue. A frame recorded
    /// inside that entry's key wait (its more prompt) would be marked after the entries still
    /// queued and come due one entry early; none is: everything that runs in a key wait records
    /// inside an [`interrupt`] (the idle poll 0x105c8e, Ready, Unready, a trapped chest),
    /// marked against an empty queue.
    pub mark: usize,
    /// The window was presented dissolving (0x1520e2): playback reveals it over its time.
    pub dissolve: bool,
}

/// Records the screen rectangle (x0, y0)..=(x1, y1) as it is now (after a present), shown after
/// `cost` ms. Sources equal to those of a frame recorded before it and not played yet (the shakes
/// of a quake that land on an offset an earlier one did, a window recorded again unchanged) share
/// that frame's allocation, the newest such frame's ([`Sources::shared`]).
pub(crate) fn capture(s: &mut State, x0: usize, y0: usize, x1: usize, y1: usize, cost: u16) {
    if x1 < x0 || y1 < y0 {
        return;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let shot = with_current_io(|io| io.draw(|fb| rect_of(fb, (x0, y0), (w, h))));
    if let Some((pixels, src)) = shot {
        let earlier = s.combat.effects.iter().rev().map(|e| &e.src);
        let src = src.hashed().shared(earlier);
        let mark = s.main.pending_print.len();
        s.combat.effects.push(Effect {
            x: x0,
            y: y0,
            w,
            h,
            pixels,
            src,
            cost,
            mark,
            dissolve: false,
        });
    }
}

/// Records the whole map window, shown after `cost` ms.
pub(crate) fn capture_window(s: &mut State, cost: u16) {
    let (x, y) = WINDOW_AT;
    capture(s, x, y, x + WINDOW.0 - 1, y + WINDOW.1 - 1, cost);
}

/// Records the whole map window presented dissolving (0x102c90 with A4-0x6578 set), revealed in
/// the dissolve's pixel order over its `cost` ms.
pub(crate) fn capture_dissolve(s: &mut State, cost: u16) {
    let n = s.combat.effects.len();
    capture_window(s, cost);
    if let Some(e) = s.combat.effects.get_mut(n) {
        e.dissolve = true;
    }
}

/// Records `cost` ms of drawing that puts nothing new on the screen (a render into the back
/// buffer).
pub(crate) fn capture_pause(s: &mut State, cost: u16) {
    if with_current_io(|_| ()).is_none() {
        return;
    }
    let mark = s.main.pending_print.len();
    s.combat.effects.push(Effect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        pixels: Vec::new(),
        src: Sources::default(),
        cost,
        mark,
        dissolve: false,
    });
}

/// Port: one queued entry was printed ([`crate::main_loop::flush_prints`], after it ran): the
/// frames recorded after it are one entry nearer, and those whose entries are all printed are
/// due (mark 0). A flush that leaves the frames for later (the end of turn, Get) leaves them
/// due, so at the next playback they come before anything queued since.
pub(crate) fn printed_one(s: &mut State) {
    for e in &mut s.combat.effects {
        e.mark = e.mark.saturating_sub(1);
    }
}

/// Port: what an action run inside a key wait sets aside of the print it interrupts
/// ([`interrupt`]): the queued lines, the frames recorded among them that are not due yet and
/// Attack's held newline ([`crate::combat::CombatState::swallow`]).
///
/// The original prints and draws synchronously. An action that the key read 0x10790a runs
/// while a print waits for its more key (0x107f4c in 0x107e5a), such as Ready, Unready, a
/// trapped chest clicked in the inventory, or the idle refresh of 0x105c8e, prints and draws
/// right there: after whatever the interrupted code drew before the prompt, and before the
/// print's other lines, which come after the key. The port queues lines and frames, so the
/// action shows the interrupted frames that are due, sets the rest aside, plays its own in
/// place ([`play_effects`]) and puts the interrupted ones back ([`resume`]).
#[must_use]
pub(crate) struct Interrupted {
    pending: Vec<crate::main_loop::Pending>,
    effects: Vec<Effect>,
    swallow: bool,
}

/// Shows the interrupted print's frames whose lines are printed (mark 0: the original drew them
/// before the prompt), then sets its queue, its other frames and its held newline aside
/// ([`Interrupted`]).
pub(crate) async fn interrupt(state: &RefCell<State>, io: &Io) -> Interrupted {
    let outer = {
        let mut s = state.borrow_mut();
        let (due, later) = std::mem::take(&mut s.combat.effects)
            .into_iter()
            .partition(|e| e.mark == 0);
        s.combat.effects = due;
        Interrupted {
            pending: std::mem::take(&mut s.main.pending_print),
            effects: later,
            swallow: std::mem::replace(&mut s.combat.swallow, false),
        }
    };
    // Nothing is queued and no newline held: this plays the due frames and prints nothing.
    play_effects(state, io).await;
    outer
}

/// Plays whatever the action still left (its lines and frames came before the interrupted
/// print's rest), then puts the interrupted print back exactly as it was.
pub(crate) async fn resume(state: &RefCell<State>, io: &Io, outer: Interrupted) {
    play_effects(state, io).await;
    let mut s = state.borrow_mut();
    s.main.pending_print.extend(outer.pending);
    s.combat.effects.extend(outer.effects);
    s.combat.swallow = outer.swallow;
}

/// Prints the first `n` queued messages and keeps the rest queued.
async fn flush_first(state: &RefCell<State>, io: &Io, n: usize) {
    if n == 0 {
        return;
    }
    let rest = {
        let mut s = state.borrow_mut();
        let q = &mut s.main.pending_print;
        let n = n.min(q.len());
        q.split_off(n)
    };
    crate::main_loop::flush_prints(state, io).await;
    let mut s = state.borrow_mut();
    let mut q = rest;
    q.append(&mut s.main.pending_print);
    s.main.pending_print = q;
}

/// Prints the first `n` queued entries one by one, each message after its time ([`PRINT_MS`]).
async fn print_paced(state: &RefCell<State>, io: &Io, n: usize) {
    for _ in 0..n {
        let text = matches!(
            state.borrow().main.pending_print.first(),
            Some(
                crate::main_loop::Pending::Print(..)
                    | crate::main_loop::Pending::PrintSwallowed(..)
            )
        );
        if text {
            pace(state, io, PRINT_MS).await;
        }
        flush_first(state, io, 1).await;
    }
}

/// Plays the recorded frames in order with the messages queued between them, then the rest of
/// the messages: each after its time on the engine clock ([`pace`]). A no-op when nothing was
/// recorded or queued.
///
/// The synchronous code that recorded the frames left its last picture on the screen (the
/// window renders present as they go); the original showed the window as it last was until its
/// first new picture was drawn, so the playback starts from the window as last published and
/// puts the step's last picture back after the recorded ones. An action run inside a more
/// prompt of the lines before a frame ([`interrupt`]) that shows frames of its own ends on the
/// window as it leaves it, which the game then has: that becomes the picture to end on, not
/// the one taken before it (the frames still to come are pictures made before it, as recorded).
pub async fn play_effects(state: &RefCell<State>, io: &Io) {
    let effects = std::mem::take(&mut state.borrow_mut().combat.effects);
    let mut last = effects
        .iter()
        .any(|e| e.w > 0 && e.h > 0)
        .then(|| io.draw(|fb| window_of(fb)));
    if last.is_some() {
        let shown = io.published(window_of);
        io.draw(|fb| put_window(fb, &shown));
    }
    // The pictures are compared by their colours only: the sources ride along.
    let same = |a: &(Vec<u8>, Sources), b: &(Vec<u8>, Sources)| a.0 == b.0;
    let mut printed = 0;
    for e in effects {
        if e.mark > printed {
            let before = state.borrow().combat.shown;
            print_paced(state, io, e.mark - printed).await;
            printed = e.mark;
            if last.is_some() && state.borrow().combat.shown != before {
                last = Some(io.draw(|fb| window_of(fb)));
            }
        }
        if e.dissolve && (e.w, e.h) == WINDOW {
            reveal(state, io, &e).await;
        } else {
            pace(state, io, e.cost).await;
        }
        if e.w == 0 || e.h == 0 {
            continue;
        }
        io.draw(|fb| {
            for r in 0..e.h {
                for c in 0..e.w {
                    let k = r * e.w + c;
                    put(fb, (e.x + c, e.y + r), e.pixels[k], e.src.at(k));
                }
            }
        });
        io.present();
        state.borrow_mut().combat.shown += 1;
    }
    if let Some(last) = last {
        if !same(&io.draw(|fb| window_of(fb)), &last) {
            // The last recorded frame is seen before the step's own last picture replaces it.
            pace(state, io, 0).await;
            io.draw(|fb| put_window(fb, &last));
            io.present();
        } else if last.1.0.is_some() {
            // The same colours, but the frame put up may name other images (or none): the last
            // picture's sources go back, published with the plane alone, so the frames and their
            // times are those without the plane (spec 07 HD0).
            io.draw(|fb| put_window(fb, &last));
            io.present_sources();
        }
    }
    let rest = state.borrow().main.pending_print.len();
    print_paced(state, io, rest).await;
    let mut s = state.borrow_mut();
    if s.combat.swallow {
        s.combat.swallow = false;
        s.ui.text.skip_newline = true;
    }
}

/// The `w` × `h` pixels from (x, y), row-major, and their sources when `fb` has a source plane.
fn rect_of(
    fb: &crate::Framebuffer,
    (x0, y0): (usize, usize),
    (w, h): (usize, usize),
) -> (Vec<u8>, Sources) {
    let at = (y0..y0 + h).flat_map(|y| (x0..x0 + w).map(move |x| (x, y)));
    let pixels = at.clone().map(|(x, y)| fb.get(x, y).unwrap_or(0)).collect();
    let src = fb.src().map(|_| {
        at.map(|(x, y)| fb.src_at(x, y).unwrap_or_default())
            .collect()
    });
    (pixels, Sources::new(src))
}

/// The map window's pixels (with its border), row-major, with their sources ([`rect_of`]).
fn window_of(fb: &crate::Framebuffer) -> (Vec<u8>, Sources) {
    rect_of(fb, WINDOW_AT, WINDOW)
}

/// Puts back pixel `(x, y)` in `colour`, naming `src` when there is one (else classic).
fn put(
    fb: &mut crate::Framebuffer,
    (x, y): (usize, usize),
    colour: u8,
    src: Option<u6_render::Src>,
) {
    match src {
        Some(src) => fb.set_src(x, y, colour, src),
        None => fb.set(x, y, colour),
    }
}

fn put_window(fb: &mut crate::Framebuffer, (pixels, src): &(Vec<u8>, Sources)) {
    let (x0, y0) = WINDOW_AT;
    for y in 0..WINDOW.1 {
        for x in 0..WINDOW.0 {
            let k = y * WINDOW.0 + x;
            put(fb, (x0 + x, y0 + y), pixels[k], src.at(k));
        }
    }
}

/// A window presented dissolving, as the original shows it: the old picture while the window
/// renders ([`WINDOW_MS`]), then its pixels in the dissolve's order (0x1520e2: 160 × 160 from
/// (8, 8) in Original; spec 13 Decision 7, [`crate::gfx::blit::MapDissolve`]: that order's steps
/// over the shown map) at an even rate of steps up to the end of its time.
async fn reveal(state: &RefCell<State>, io: &Io, e: &Effect) {
    let taps = state.borrow().gfx.tables.dissolve_taps;
    let order = crate::gfx::blit::MapDissolve::new(&taps);
    let steps = order.steps();
    let wait = schedule(state, io, e.cost);
    let render = u64::from(WINDOW_MS.min(e.cost));
    let mut done = 0;
    loop {
        let (now, tick) = display_frame(io).await;
        let due = wait.at(tick);
        let begin = due - u64::from(e.cost) + render;
        let upto = if now >= due {
            steps
        } else {
            let part = now.saturating_sub(begin) * steps as u64 / (due - begin).max(1);
            part as usize
        };
        if upto > done {
            io.draw(|fb| {
                for step in done..upto {
                    for (x, y) in order.step(step) {
                        let x = usize::from(x) - WINDOW_AT.0;
                        let y = usize::from(y) - WINDOW_AT.1;
                        let k = y * e.w + x;
                        put(fb, (e.x + x, e.y + y), e.pixels[k], e.src.at(k));
                    }
                }
            });
            io.present();
            done = upto;
        }
        if now >= due {
            break;
        }
    }
    finish(state, io, &wait);
}

/// Original: 0x13242e (ui: grow the flight's changed rectangle A4+0x1900.. to the 16 × 16
/// sprite centred on (x, y), clipped to the window 8..0xa7; the shown map [`SHOWN`], spec 13)
pub fn extend_dirty(s: &mut State, x: i16, y: i16) {
    let (x0, y0, x1, y1) = SHOWN_PX;
    let d = &mut s.combat.dirty;
    if x < x0 + 8 {
        d[0] = x0;
    } else if x - 8 < d[0] {
        d[0] = x - 8;
    }
    if y < y0 + 8 {
        d[1] = y0;
    } else if y - 8 < d[1] {
        d[1] = y - 8;
    }
    if x > x1 - 8 {
        d[2] = x1;
    } else if x + 8 > d[2] {
        d[2] = x + 8;
    }
    if y > y1 - 8 {
        d[3] = y1;
    } else if y + 8 > d[3] {
        d[3] = y + 8;
    }
}

fn back_blit(s: &State, tile: u16, x: u16, y: u16) {
    with_current_io(|io| io.draw_back(|fb| blit_back(&s.gfx, fb, tile, x, y)));
}

/// Original: 0x1324ae (ui: redraw the window border (tiles 0x1b0..0x1b7) along the edges the
/// rectangle (x0, y0)..(x1, y1) touches: the left edge for x0 < 16, the top for y0 < 16, the
/// right for x1 > 0x9f, the bottom for y1 > 0x9f; within 8 pixels of [`SHOWN`]'s edges, spec 13)
///
/// The tiles, their places and their order are the render's ([`crate::gfx::view::border_tiles`]:
/// the corners, then the edge cells from the first).
pub fn draw_borders(s: &State, x0: i16, y0: i16, x1: i16, y1: i16) {
    let (sx0, sy0, sx1, sy1) = SHOWN_PX;
    let (left, top) = (x0 < sx0 + 8, y0 < sy0 + 8);
    let (right, bottom) = (x1 > sx1 - 8, y1 > sy1 - 8);
    for (tile, x, y) in crate::gfx::view::border_tiles() {
        let on = match tile {
            0x1b0 => left && top,
            0x1b2 => right && top,
            0x1b3 => left && bottom,
            0x1b5 => right && bottom,
            0x1b1 => top,
            0x1b4 => bottom,
            0x1b6 => left,
            0x1b7 => right,
            _ => false,
        };
        if on {
            back_blit(s, tile, x, y);
        }
    }
}

/// Original: 0x1325e4 (ui: redraw what lies over a flight in the rectangle (x0, y0)..(x1, y1):
/// per window cell the draw-list tiles with flags2 bit 4 (on top), darkness (tile 0xff) on a
/// cell whose base is 0xff or 0x1bc, then the border)
///
/// Port (spec 18, [`crate::Io::set_sight_memory`] on): a cell the window render shows from the
/// remembered sight gets its grey layers first, in place of the darkness, then its tiles on top
/// ([`crate::gfx::view::blit_remembered`]), as the render draws it.
///
/// Cells by view cell from the map's corner, `row * VIEW_W + col` (spec 13 C2).
pub fn redraw_over(s: &State, x0: i16, y0: i16, x1: i16, y1: i16) {
    let d = &s.objects.draw;
    let (mx, my) = (MAP_X as i16, MAP_Y as i16);
    let at = |col: i16, row: i16| ((mx + col * 16) as u16, (my + row * 16) as u16);
    for col in ((x0 - mx) >> 4)..=((x1 - mx) >> 4) {
        for row in ((y0 - my) >> 4)..=((y1 - my) >> 4) {
            let cell = (row * VIEW_W as i16 + col) as usize;
            let remembered = with_current_io(|io| {
                io.sight_memory()
                    && io.draw_back(|fb| crate::gfx::view::blit_remembered(s, fb, col, row))
            });
            let remembered = remembered.unwrap_or(false);
            let mut n = d.cell.get(cell).copied().unwrap_or(0);
            let mut guard = 0;
            // At most the pool's nodes (256 in Original; spec 13 Design 2 sizes it in Wide).
            while n != 0 && guard < crate::objects::DRAW_NODES {
                let t = d.node_tile[usize::from(n)];
                if t != 0 {
                    let tile = t & 0x7ff;
                    let f2 = s.tables.tileflags.flags2.get(usize::from(tile)).copied();
                    if f2.unwrap_or(0) & 0x10 != 0 {
                        let (x, y) = at(col, row);
                        back_blit(s, tile, x, y);
                    }
                }
                n = d.node_next[usize::from(n)];
                guard += 1;
            }
            let base = d.base.get(cell).copied().unwrap_or(0);
            if !remembered && (base == 0xff || base == 0x1bc) {
                let (x, y) = at(col, row);
                back_blit(s, 0xff, x, y);
            }
        }
    }
    draw_borders(s, x0, y0, x1, y1);
}

/// Where 0x132706 draws a missile at map pixel (px, py) (16 a cell) for the view centre `c`: its
/// frame pixel, when that lies in the shown map (`px - (c - 5)·16` in 8..0xa8 in Original; spec
/// 13 Decision 5: from the view's corner, [`HALF_W`] and [`HALF_H`] cells off the centre, inside
/// [`SHOWN`]).
pub(crate) fn on_screen(c: u6_formats::position::Position, px: i16, py: i16) -> Option<(i16, i16)> {
    let (left, top) = (c.x as i16 - HALF_W as i16, c.y as i16 - HALF_H as i16);
    let sx = px.wrapping_sub(left * 16).wrapping_add(MAP_X as i16);
    let sy = py.wrapping_sub(top * 16).wrapping_add(MAP_Y as i16);
    let (x0, y0, x1, y1) = SHOWN_PX;
    ((x0..=x1).contains(&sx) && (y0..=y1).contains(&sy)).then_some((sx, sy))
}

/// Per-missile state of a flight (the 16-entry stack arrays of 0x132706).
#[derive(Clone, Copy, Debug, Default)]
struct Missile {
    /// -0x40 / -0x60: position in pixels of the map.
    px: i16,
    py: i16,
    /// -0x1a0 / -0x1c0: the tile it is on.
    tx: i16,
    ty: i16,
    /// -0x80: steps along the major axis (the flight ends for it at that step).
    steps: i16,
    /// -0xa0: steps along the minor axis.
    minor: i16,
    /// -0xe0 / -0x120: x and y of the straight (0) and diagonal (1) step.
    sx: [i16; 2],
    sy: [i16; 2],
    /// -0x140: the error term; -0x180: what each choice adds to it.
    err: i16,
    add: [i16; 2],
    /// -0x1e0: sprite angle.
    angle: i16,
    /// -0x240 / -0x260: where its sprite was last drawn on the screen.
    drawn: (i16, i16),
}

/// Flight flags (the word at 0xe(A5) of 0x132706).
pub mod flight {
    /// The missile flies back from the target to the shooter (boomerang).
    pub const RETURN: u16 = 1;
    /// Sprites are drawn straight into the window without saving what is under them.
    pub const TRAIL: u16 = 2;
    /// One unturned sprite (slot 15) for every missile.
    pub const SHARED: u16 = 8;
}

/// Original: 0x132706 (ui: fly `n` missiles of `tile` from tile (x0, y0) toward (xs[i], ys[i]))
///
/// Bresenham in pixels (16 per tile), one pixel step per round along the major axis; a missile
/// stops at the first tile past `range` (the A4-0x69a0 distance from (x0, y0)), at a map tile
/// with flags2 bit 5 (bit 3 lets it through next to the shooter), or at an object whose tile has
/// flags2 bit 5; stopping one missile ends the round for the others. Actors it passes (not the
/// lying 0x165 frame 0, not swallowed) are listed in A4+0x186e. Sprites (turned by the A4-0x64f4
/// angle table unless [`flight::SHARED`]) go into the back buffer over saved backgrounds and the
/// changed rectangle is copied to the screen each round ([`capture`]d for playback). At the end
/// `xs` / `ys` hold where each missile stopped (a [`flight::RETURN`] flight keeps them), the
/// first one's object (seen by the shooter A4-0x69a2) goes to A4+0x185c; returns 1 when every
/// missile reached its target.
#[allow(clippy::too_many_arguments)]
pub fn fly(
    s: &mut State,
    x0: i16,
    y0: i16,
    xs: &mut [i16],
    ys: &mut [i16],
    n: i16,
    range: i16,
    tile: u16,
    flags: u16,
) -> i16 {
    with_current_io(|io| crate::gfx::render_window(s, io, false));
    capture_pause(s, FLIGHT_START_MS);
    let n = n.clamp(0, 16) as usize;
    let mut passed = [0u16; 32];
    let mut m = [Missile::default(); 16];
    m[0].tx = x0;
    m[0].ty = y0;
    s.combat.landing = (x0, y0);
    let mut reached: i16 = 1;
    if flags & flight::RETURN != 0 {
        m[0].tx = xs[0];
        xs[0] = x0;
        std::mem::swap(&mut m[0].ty, &mut ys[0]);
    }
    m[0].px = m[0].tx * 16 + 8;
    m[0].py = m[0].ty * 16 + 8;
    let t = s.combat.tables.clone();
    let kind = t.missile_tiles.iter().position(|&k| k == tile).unwrap_or(0);
    let step = if flags & flight::TRAIL != 0 {
        6
    } else {
        i16::from(at(&t.missile_step, kind))
    };
    let (cx, cy) = (
        u16::from(at(&t.missile_cx, kind)),
        u16::from(at(&t.missile_cy, kind)),
    );
    let (sx0, sy0) = (m[0].tx, m[0].ty);
    if tile != 0 {
        if flags & flight::SHARED != 0 {
            super::seams::sprite_prepare(s, 0xf, tile, 0, 0x7f, 0x7f);
        } else {
            for i in 0..n {
                let a = i16::from(t.angle(xs[i].wrapping_sub(sx0), ys[i].wrapping_sub(sy0)));
                m[i].angle = a.wrapping_sub(i16::from(at(&t.missile_angle, kind)));
                let slot = if flags & flight::TRAIL != 0 { i } else { i + 8 } as u16;
                super::seams::sprite_prepare(s, slot, tile, m[i].angle as u16, cx, cy);
            }
        }
    }
    let mut longest: i16 = 0;
    for i in 0..n {
        let first = m[0];
        let mi = &mut m[i];
        mi.px = first.px;
        mi.py = first.py;
        mi.tx = first.tx;
        mi.ty = first.ty;
        let mut major = xs[i].wrapping_sub(first.tx).wrapping_mul(16);
        let mut minor = ys[i].wrapping_sub(first.ty).wrapping_mul(16);
        mi.sy[0] = 0;
        let sx = if major < 0 {
            major = major.wrapping_neg();
            -1
        } else {
            1
        };
        mi.sx = [sx, sx];
        mi.sy[1] = if minor < 0 {
            minor = minor.wrapping_neg();
            -1
        } else {
            1
        };
        if major < minor {
            std::mem::swap(&mut major, &mut minor);
            mi.sy[0] = mi.sy[1];
            mi.sx[1] = mi.sx[0];
            mi.sx[0] = 0;
        }
        mi.steps = major;
        mi.minor = minor;
        if major > longest {
            longest = major;
        }
        mi.err = minor.wrapping_add(minor).wrapping_sub(major);
        mi.add = [
            minor.wrapping_add(minor),
            minor.wrapping_sub(major).wrapping_mul(2),
        ];
    }
    let spin = i16::from(at(&t.missile_spin, kind));
    let mut round: i16 = 0;
    while round < longest {
        s.combat.dirty = NO_DIRTY;
        let mut visible = false;
        'missiles: for i in 0..n {
            if round >= m[i].steps {
                if round != 0 {
                    let (dx, dy) = m[i].drawn;
                    extend_dirty(s, dx, dy);
                }
                continue;
            }
            let k = usize::from(m[i].err >= 0);
            m[i].err = m[i].err.wrapping_add(m[i].add[k]);
            m[i].px = m[i].px.wrapping_add(m[i].sx[k]);
            m[i].py = m[i].py.wrapping_add(m[i].sy[k]);
            if tile != 0 && step != 0 && round % step == 0 {
                if let Some((sx, sy)) = on_screen(s.map.centre, m[i].px, m[i].py) {
                    visible = true;
                    extend_dirty(s, sx, sy);
                    if flags & flight::TRAIL != 0 {
                        super::seams::sprite_draw(s, i as u16, sx as u16, sy as u16);
                    } else {
                        if round != 0 {
                            let (dx, dy) = m[i].drawn;
                            extend_dirty(s, dx, dy);
                        }
                        m[i].drawn = (sx, sy);
                    }
                }
                if spin != 0 {
                    m[i].angle = m[i].angle.wrapping_add(spin);
                    if flags & (flight::TRAIL | flight::SHARED) == 0 {
                        let a = m[i].angle as u16;
                        super::seams::sprite_prepare(s, i as u16 + 8, tile, a, cx, cy);
                    }
                }
            }
            let (ntx, nty) = (m[i].px >> 4, m[i].py >> 4);
            if ntx == m[i].tx && nty == m[i].ty {
                continue;
            }
            let ddx = ntx.wrapping_sub(x0).wrapping_abs();
            let ddy = nty.wrapping_sub(y0).wrapping_abs();
            if ddx > 7 || ddy > 7 || i16::from(t.distance_at(ddx as usize, ddy as usize)) > range {
                m[i].steps = round;
                break 'missiles;
            }
            m[i].tx = ntx;
            m[i].ty = nty;
            let z = u16::from(s.map.centre.z);
            let mt = crate::actors::seams::tile_at(s, ntx as u16, nty as u16, z);
            let f2 = |s: &State, tile: u16| {
                s.tables
                    .tileflags
                    .flags2
                    .get(usize::from(tile))
                    .copied()
                    .unwrap_or(0)
            };
            let near = ddx <= 1 && ddy <= 1 && f2(s, u16::from(mt)) & 0x08 != 0;
            if !near && f2(s, u16::from(mt)) & 0x20 != 0 {
                m[i].steps = round;
                break 'missiles;
            }
            let mut o =
                crate::objects::begin_at(&mut s.objects, &s.tables, ntx as u16, nty as u16, z);
            while o >= 0 {
                let w = s.objects.type_frame[o as usize];
                if o < 0x100 {
                    let lying = w & 0x3ff == 0x165 && w >> 10 == 0;
                    if !lying && s.actors.attrs.movement_flags[o as usize] & 0x10 == 0 {
                        passed[(o >> 3) as usize] |= 1 << (o & 7);
                    }
                }
                let base = s.tables.basetile.base.get(usize::from(w & 0x3ff)).copied();
                let part_tile = base
                    .unwrap_or(0)
                    .wrapping_add(w >> 10)
                    .wrapping_sub(s.objects.at.part);
                if f2(s, part_tile) & 0x20 != 0 {
                    break;
                }
                o = crate::objects::next_at(&mut s.objects, &s.tables);
            }
            if o >= 0 {
                m[i].steps = round;
                break 'missiles;
            }
            if i == 0 {
                s.combat.landing = (m[0].tx, m[0].ty);
            }
        }
        if visible {
            if round & 3 == 0 {
                crate::main_loop::anim_step(s);
            }
            let draws = flags & flight::TRAIL == 0;
            if draws {
                for (i, mi) in m.iter().enumerate().take(n) {
                    if round < mi.steps || flags & flight::SHARED != 0 {
                        let (dx, dy) = mi.drawn;
                        super::seams::sprite_save(s, i as u16, dx as u16, dy as u16);
                    }
                }
                for (i, mi) in m.iter().enumerate().take(n) {
                    let (dx, dy) = mi.drawn;
                    if flags & flight::SHARED != 0 {
                        super::seams::sprite_draw(s, 0xf, dx as u16, dy as u16);
                    } else if round < mi.steps {
                        super::seams::sprite_draw(s, i as u16 + 8, dx as u16, dy as u16);
                    }
                }
            }
            let [dx0, dy0, dx1, dy1] = s.combat.dirty;
            if dx0 <= dx1 && dy0 <= dy1 {
                redraw_over(s, dx0, dy0, dx1, dy1);
                with_current_io(|io| {
                    io.with_buffers(|back, front| {
                        let w = |v: i16| v as u16;
                        copy_rect(back, front, w(dx0), w(dy0), w(dx1), w(dy1), w(dx0), w(dy0));
                    })
                });
                let (cx0, cx1) = ((dx0 as usize) & !7, (dx1 as usize) | 7);
                capture(s, cx0, dy0 as usize, cx1, dy1 as usize, FLIGHT_STEP_MS);
            }
            if draws {
                for (i, mi) in m.iter().enumerate().take(n) {
                    if round < mi.steps || flags & flight::SHARED != 0 {
                        let (dx, dy) = mi.drawn;
                        super::seams::sprite_draw(s, i as u16, dx as u16, dy as u16);
                    }
                }
            }
        }
        round += 1;
    }
    if tile != 0 && flags & (flight::TRAIL | flight::SHARED) == 0 {
        with_current_io(|io| crate::gfx::refresh_view(s, io));
        capture_window(s, REFRESH_MS);
    }
    for i in 0..n {
        let moved_x = m[i].tx != xs[i];
        let moved_y = m[i].ty != ys[i];
        if moved_x || (moved_y && flags & flight::RETURN == 0) {
            xs[i] = m[i].tx;
            ys[i] = m[i].ty;
            reached = 0;
            if i == 0 && s.combat.shooter >= 0 {
                let shooter = s.combat.shooter;
                s.combat.hit = super::leaves::actor_at(s, shooter, m[0].tx, m[0].ty);
            }
        }
    }
    let mut list = Vec::new();
    'words: for (w, bits) in passed.iter().enumerate() {
        for b in 0..8 {
            if bits & (1 << b) != 0 {
                list.push((w * 8 + b) as i16);
            }
            if list.len() > 0x3e {
                break 'words;
            }
        }
    }
    list.push(-1);
    s.combat.passed = list;
    reached
}

/// Original: 0x1331b4 (ui: a blast of 16 sprites of `tile` from (x, y) out to the A4-0x63e6 /
/// -0x63d6 offsets, range 3, drawn as trails; returns the actors it reached, A4+0x186e: those
/// the flights passed, then those on (x, y) up to the first object that stops missiles)
pub fn blast(s: &mut State, x: i16, y: i16, tile: u16) -> Vec<i16> {
    let mut xs = [0i16; 16];
    let mut ys = [0i16; 16];
    for i in 0..16 {
        xs[i] = x.wrapping_add(i16::from(at(&s.combat.tables.blast_dx, i) as i8));
        ys[i] = y.wrapping_add(i16::from(at(&s.combat.tables.blast_dy, i) as i8));
    }
    fly(s, x, y, &mut xs, &mut ys, 16, 3, tile, 6);
    let mut list = s.combat.passed.clone();
    let k = list.iter().take(0x3e).take_while(|&&a| a >= 0).count();
    list.truncate(k);
    let z = u16::from(s.map.centre.z);
    let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
    while list.len() < 0x3f && o >= 0 {
        let w = s.objects.type_frame[o as usize];
        if o < 0x100 {
            let lying = w & 0x3ff == 0x165 && w >> 10 == 0;
            if !lying && s.actors.attrs.movement_flags[o as usize] & 0x10 == 0 {
                list.push(o);
            }
        }
        let base = s.tables.basetile.base.get(usize::from(w & 0x3ff)).copied();
        let t = base
            .unwrap_or(0)
            .wrapping_add(w >> 10)
            .wrapping_sub(s.objects.at.part);
        if s.tables
            .tileflags
            .flags2
            .get(usize::from(t))
            .copied()
            .unwrap_or(0)
            & 0x20
            != 0
        {
            break;
        }
        o = crate::objects::next_at(&mut s.objects, &s.tables);
    }
    list.push(-1);
    s.combat.passed = list.clone();
    list
}

/// A `printf` argument of a combat message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    Str(Vec<u8>),
    Word(i16),
}

/// One combat message as queued: the image address of its format and the arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    pub at: u32,
    pub args: Vec<Text>,
}

/// Messages [`CombatState::said`](super::CombatState) keeps (the last ones).
const SAID_KEPT: usize = 64;

/// [ui] 0x108658 with the string at image address `at` of the user's `game` as the format:
/// queued for the command loop like the actor code's lines, and kept in `combat.said`.
///
/// While Attack's newline is held (`combat.swallow`) the line's leading newlines are dropped
/// when it is printed, and the newline stays held while the English is nothing but newlines.
/// Port (spec 08, 2026-10-24): the format is queued whole
/// ([`crate::main_loop::Pending::PrintSwallowed`]) so the locale layer finds it; the newlines
/// are dropped from the text it puts ([`crate::ui::printf::printf_swallowed`]).
pub(crate) fn say(s: &mut State, at: u32, args: Vec<Text>) {
    let said = &mut s.combat.said;
    if said.len() >= SAID_KEPT {
        said.remove(0);
    }
    said.push(Said {
        at,
        args: args.clone(),
    });
    let fmt = with_current_io(|io| crate::main_loop::game_string(io, at));
    if let Some(fmt) = fmt {
        let swallowed = s.combat.swallow;
        if swallowed && fmt.iter().any(|&b| b != b'\n') {
            s.combat.swallow = false;
        }
        let args = args
            .into_iter()
            .map(|a| match a {
                Text::Str(b) => crate::main_loop::PrintArg::Str(b),
                Text::Word(w) => crate::main_loop::PrintArg::Word(w as u16),
            })
            .collect();
        if swallowed {
            let q = &mut s.main.pending_print;
            q.push(crate::main_loop::Pending::PrintSwallowed(fmt, args));
        } else {
            crate::main_loop::queue_print(s, fmt, args);
        }
    }
}

/// The display name of an object (0x10eef8), as a message argument, after the time the original
/// takes to read it out of look.lzd ([`NAME_MS`]). Port (spec 08, slice K3): the locale pack's
/// object name when it has one ([`crate::objects::shown_name`]).
pub(crate) fn name(s: &mut State, o: i16) -> Text {
    capture_pause(s, NAME_MS);
    Text::Str(crate::objects::shown_name(s, o).into_bytes())
}

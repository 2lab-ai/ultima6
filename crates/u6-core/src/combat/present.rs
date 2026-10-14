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
use crate::gfx::blit::{blit_back, copy_rect};
use crate::state::State;

use super::tables::at;

/// Milliseconds of one PAL frame (the original's display runs at 50 Hz).
pub const FRAME_MS: u16 = 20;
/// [replaced] Time of the hit flash 0x1230ce (draw lists, the window with tile 0x101 rendered
/// and copied, draw lists again): 311 ms, 15.56 PAL frames, the median of 20 calls traced in the
/// oracle's start fight (15.31..15.95; tools/oracle/trace.mjs). fight.py's CPU samples give 16
/// (n = 45).
pub const FLASH_MS: u16 = 311;
/// [replaced] Time of a render of the window into the back buffer (0x102c90 without the frame)
/// and what a flight does before its first step: 129 ms, 6.45 PAL frames, the median of the 29
/// traced flights that drew no step (6.19..6.88; the CPU samples of 87 flights give 6).
pub const RENDER_MS: u16 = 129;
/// [replaced] Time of a render of the window with its frame, copied to the screen (0x102c90 with
/// D0 = 1): 8.74 PAL frames, timed at the prompt (tools/oracle/timeit.mjs render).
pub const WINDOW_MS: u16 = 175;
/// A flight's first render (0x102c90 at the top of 0x132706, before the first step).
pub const FLIGHT_START_MS: u16 = RENDER_MS;
/// [replaced] Time of one drawn step of a flight (the rounds up to it, the sprites saved and
/// drawn, what lies over them, the rectangle copied, 0x151fe6): 46 ms, 2.31 PAL frames. A
/// sampled estimate: fight.py's 525 frames over 227 changes of the view in the steps of the 40
/// flights that drew any (no routine boundary to trace; the traced whole flights, median 11.8
/// frames over 58, agree with start + steps + end).
pub const FLIGHT_STEP_MS: u16 = 46;
/// [replaced] Time of a view refresh with the window shown (0x102ef0: the animation tick, the
/// window rendered by 0x102c90 and copied to the screen): 11 PAL frames. A sampled estimate:
/// the median of the 40 flights whose CPU samples end in one (9..42, 29 of them 10 or 11); with
/// the view flags before it, the traced refresh after a turn
/// ([`crate::actors::turn::AFTER_TURN_MS`], 21.58).
pub const REFRESH_MS: u16 = 11 * FRAME_MS;
/// [replaced] Time of one message (0x108658 drawing into the scroll, scrolling it): 23 ms, 1.17
/// PAL frames, the mean of 63 calls traced in the oracle (tools/oracle/trace.mjs; median 0.39,
/// the ones that scroll take up to 3.5). Per call, as the port prints one queued message a call.
pub const PRINT_MS: u16 = 23;
/// [replaced] Time of an object's name read for a message (0x10eef8: 0x10ee76 decodes it out of
/// the look.lzd copy in memory, 0x161ec8): 27 ms, 1.33 PAL frames, the mean of 33 calls traced
/// in the oracle's fight (median 1.28). The first read of a boot also loads the file from the
/// floppy (133 frames): not modelled.
pub const NAME_MS: u16 = 27;
/// [replaced] Time of a window presented dissolving (0x102c90 with A4-0x6578 set: 0x1520e2
/// copies the 25,600 pixels of the window one by one): 171.07 PAL frames, timed at the prompt
/// (tools/oracle/timeit.mjs dissolve; none happens in the start fight).
pub const DISSOLVE_MS: u16 = 3421;
/// [replaced] Time of each window of the actor flash 0x132338 (the outlined actor presented
/// dissolving, then the window without it, dissolving too): half of its 345.83 PAL frames, timed
/// at the prompt on the Avatar (tools/oracle/timeit.mjs actor).
pub const ACTOR_FLASH_MS: u16 = 3458;
/// The map window with its border, in screen pixels.
const WINDOW: (usize, usize) = (176, 176);

/// Port: waits until the original would have spent `ms` more on what it draws, on the engine
/// clock, and at least one display frame, so what was presented before is published.
///
/// The waits of one run (nothing else waited in between) follow one schedule: `combat.due_ms`
/// moves on by `ms` from where the last one ended, so what a wait loses to the host's tick the
/// next one makes up, and the run takes the same engine time at any tick size. Lateness is
/// carried at most one tick (the shortest tick of the run): a host that stalls for one long
/// tick does not squeeze the waits after it. After anything else waited (a key, a prompt) the
/// schedule starts again from now.
pub async fn pace(state: &RefCell<State>, io: &Io, ms: u16) {
    let due = schedule(state, io, ms);
    loop {
        if display_frame(state, io).await >= due {
            break;
        }
    }
    state.borrow_mut().combat.paced_at_ms = io.now_ms();
}

/// The deadline of a wait of `ms` on the schedule of [`pace`] (counted, not awaited).
fn schedule(state: &RefCell<State>, io: &Io, ms: u16) -> u64 {
    let mut s = state.borrow_mut();
    let now = io.now_ms();
    let c = &mut s.combat;
    let from = if now == c.paced_at_ms {
        c.due_ms.max(now.saturating_sub(c.tick_ms))
    } else {
        c.tick_ms = u64::MAX;
        now
    };
    c.due_ms = from + u64::from(ms);
    *c.paced.entry(ms).or_default() += 1;
    c.due_ms
}

/// One display frame of a paced wait: the engine time after it, the run's shortest tick kept.
async fn display_frame(state: &RefCell<State>, io: &Io) -> u64 {
    let before = io.now_ms();
    io.vblank().await;
    let now = io.now_ms();
    if now > before {
        let t = &mut state.borrow_mut().combat.tick_ms;
        *t = (*t).min(now - before);
    }
    now
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
    /// Engine milliseconds the original spends making it: [`play_effects`] waits that long
    /// before showing it.
    pub cost: u16,
    /// Messages queued (`main.pending_print`) before it was presented.
    pub mark: usize,
    /// The window was presented dissolving (0x1520e2): playback reveals it over its time.
    pub dissolve: bool,
}

/// Records the screen rectangle (x0, y0)..=(x1, y1) as it is now (after a present), shown after
/// `cost` ms.
pub(crate) fn capture(s: &mut State, x0: usize, y0: usize, x1: usize, y1: usize, cost: u16) {
    if x1 < x0 || y1 < y0 {
        return;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let pixels = with_current_io(|io| {
        io.draw(|fb| {
            let mut p = Vec::with_capacity(w * h);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    p.push(fb.get(x, y).unwrap_or(0));
                }
            }
            p
        })
    });
    if let Some(pixels) = pixels {
        let mark = s.main.pending_print.len();
        s.combat.effects.push(Effect {
            x: x0,
            y: y0,
            w,
            h,
            pixels,
            cost,
            mark,
            dissolve: false,
        });
    }
}

/// Records the whole map window, shown after `cost` ms.
pub(crate) fn capture_window(s: &mut State, cost: u16) {
    capture(s, 0, 0, WINDOW.0 - 1, WINDOW.1 - 1, cost);
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
        cost,
        mark,
        dissolve: false,
    });
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
            Some(crate::main_loop::Pending::Print(..))
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
/// puts the step's last picture back after the recorded ones.
pub async fn play_effects(state: &RefCell<State>, io: &Io) {
    let effects = std::mem::take(&mut state.borrow_mut().combat.effects);
    let last = effects
        .iter()
        .any(|e| e.w > 0 && e.h > 0)
        .then(|| io.draw(|fb| window_of(fb)));
    if last.is_some() {
        let shown = io.published(window_of);
        io.draw(|fb| put_window(fb, &shown));
    }
    let mut printed = 0;
    for e in effects {
        if e.mark > printed {
            print_paced(state, io, e.mark - printed).await;
            printed = e.mark;
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
                    fb.set(e.x + c, e.y + r, e.pixels[r * e.w + c]);
                }
            }
        });
        io.present();
        state.borrow_mut().combat.shown += 1;
    }
    if let Some(last) = last
        && io.draw(|fb| window_of(fb)) != last
    {
        // The last recorded frame is seen before the step's own last picture replaces it.
        pace(state, io, 0).await;
        io.draw(|fb| put_window(fb, &last));
        io.present();
    }
    let rest = state.borrow().main.pending_print.len();
    print_paced(state, io, rest).await;
    let mut s = state.borrow_mut();
    if s.combat.swallow {
        s.combat.swallow = false;
        s.ui.text.skip_newline = true;
    }
}

/// The map window's pixels (with its border), row-major.
fn window_of(fb: &crate::Framebuffer) -> Vec<u8> {
    (0..WINDOW.1)
        .flat_map(|y| (0..WINDOW.0).map(move |x| (x, y)))
        .map(|(x, y)| fb.get(x, y).unwrap_or(0))
        .collect()
}

fn put_window(fb: &mut crate::Framebuffer, pixels: &[u8]) {
    for y in 0..WINDOW.1 {
        for x in 0..WINDOW.0 {
            fb.set(x, y, pixels[y * WINDOW.0 + x]);
        }
    }
}

/// A window presented dissolving, as the original shows it: the old picture while the window
/// renders ([`WINDOW_MS`]), then its pixels in the dissolve's order (0x1520e2: 160 × 160 from
/// (8, 8), [`crate::gfx::blit::dissolve_order`]) at an even rate up to the end of its time.
async fn reveal(state: &RefCell<State>, io: &Io, e: &Effect) {
    let taps = state.borrow().gfx.tables.dissolve_taps;
    let order = crate::gfx::blit::dissolve_order(160, 160, &taps);
    let due = schedule(state, io, e.cost);
    let begin = due - u64::from(e.cost) + u64::from(WINDOW_MS.min(e.cost));
    let mut done = 0;
    loop {
        let now = display_frame(state, io).await;
        let upto = if now >= due {
            order.len()
        } else {
            let part = now.saturating_sub(begin) * order.len() as u64 / (due - begin).max(1);
            part as usize
        };
        if upto > done {
            io.draw(|fb| {
                for &(c, r) in &order[done..upto] {
                    let (x, y) = (8 + usize::from(c), 8 + usize::from(r));
                    fb.set(e.x + x, e.y + y, e.pixels[y * e.w + x]);
                }
            });
            io.present();
            done = upto;
        }
        if now >= due {
            break;
        }
    }
    state.borrow_mut().combat.paced_at_ms = io.now_ms();
}

/// Original: 0x13242e (ui: grow the flight's changed rectangle A4+0x1900.. to the 16 × 16
/// sprite centred on (x, y), clipped to the window 8..0xa7)
pub fn extend_dirty(s: &mut State, x: i16, y: i16) {
    let d = &mut s.combat.dirty;
    if x < 16 {
        d[0] = 8;
    } else if x - 8 < d[0] {
        d[0] = x - 8;
    }
    if y < 16 {
        d[1] = 8;
    } else if y - 8 < d[1] {
        d[1] = y - 8;
    }
    if x > 0x9f {
        d[2] = 0xa7;
    } else if x + 8 > d[2] {
        d[2] = x + 8;
    }
    if y > 0x9f {
        d[3] = 0xa7;
    } else if y + 8 > d[3] {
        d[3] = y + 8;
    }
}

fn back_blit(s: &State, tile: u16, x: u16, y: u16) {
    with_current_io(|io| io.draw_back(|fb| blit_back(&s.gfx, fb, tile, x, y)));
}

/// Original: 0x1324ae (ui: redraw the window border (tiles 0x1b0..0x1b7) along the edges the
/// rectangle (x0, y0)..(x1, y1) touches)
pub fn draw_borders(s: &State, x0: i16, y0: i16, x1: i16, y1: i16) {
    let (left, top, right, bottom) = (x0 < 16, y0 < 16, x1 > 0x9f, y1 > 0x9f);
    if left && top {
        back_blit(s, 0x1b0, 0, 0);
    }
    if right && top {
        back_blit(s, 0x1b2, 0xa0, 0);
    }
    if left && bottom {
        back_blit(s, 0x1b3, 0, 0xa0);
    }
    if right && bottom {
        back_blit(s, 0x1b5, 0xa0, 0xa0);
    }
    for i in 1..10u16 {
        if top {
            back_blit(s, 0x1b1, i * 16, 0);
        }
        if bottom {
            back_blit(s, 0x1b4, i * 16, 0xa0);
        }
        if left {
            back_blit(s, 0x1b6, 0, i * 16);
        }
        if right {
            back_blit(s, 0x1b7, 0xa0, i * 16);
        }
    }
}

/// Original: 0x1325e4 (ui: redraw what lies over a flight in the rectangle (x0, y0)..(x1, y1):
/// per window cell the draw-list tiles with flags2 bit 4 (on top), darkness (tile 0xff) on a
/// cell whose base is 0xff or 0x1bc, then the border)
pub fn redraw_over(s: &State, x0: i16, y0: i16, x1: i16, y1: i16) {
    let d = &s.objects.draw;
    for col in (x0 >> 4)..=(x1 >> 4) {
        for row in (y0 >> 4)..=(y1 >> 4) {
            let cell = (row * 11 + col) as usize;
            let mut n = d.cell.get(cell).copied().unwrap_or(0);
            let mut guard = 0;
            while n != 0 && guard < 256 {
                let t = d.node_tile[usize::from(n)];
                if t != 0 {
                    let tile = t & 0x7ff;
                    let f2 = s.tables.tileflags.flags2.get(usize::from(tile)).copied();
                    if f2.unwrap_or(0) & 0x10 != 0 {
                        back_blit(s, tile, (col * 16) as u16, (row * 16) as u16);
                    }
                }
                n = d.node_next[usize::from(n)];
                guard += 1;
            }
            let base = d.base.get(cell).copied().unwrap_or(0);
            if base == 0xff || base == 0x1bc {
                back_blit(s, 0xff, (col * 16) as u16, (row * 16) as u16);
            }
        }
    }
    draw_borders(s, x0, y0, x1, y1);
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
        s.combat.dirty = [0xa8, 0xa8, 7, 7];
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
                let c = s.map.centre;
                let sx = m[i].px.wrapping_sub((c.x as i16 - 5) * 16);
                let sy = m[i].py.wrapping_sub((c.y as i16 - 5) * 16);
                if (8..0xa8).contains(&sx) && (8..0xa8).contains(&sy) {
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
    if let Some(mut fmt) = fmt {
        if s.combat.swallow {
            let n = fmt.iter().take_while(|&&b| b == b'\n').count();
            fmt.drain(..n);
            if !fmt.is_empty() {
                s.combat.swallow = false;
            }
        }
        let args = args
            .into_iter()
            .map(|a| match a {
                Text::Str(b) => crate::main_loop::PrintArg::Str(b),
                Text::Word(w) => crate::main_loop::PrintArg::Word(w as u16),
            })
            .collect();
        crate::main_loop::queue_print(s, fmt, args);
    }
}

/// The display name of an object (0x10eef8), as a message argument, after the time the original
/// takes to read it out of look.lzd ([`NAME_MS`]).
pub(crate) fn name(s: &mut State, o: i16) -> Text {
    capture_pause(s, NAME_MS);
    Text::Str(crate::objects::display_name(s, o).into_bytes())
}

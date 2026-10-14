//! The introduction story of the main menu (0x1078e6, `intro.new`, song 3, palette 2): the
//! Avatar comes through the portal onto the gargoyles' plain, is bound to their altar, and the
//! companions come through a moongate and carry him off.
//!
//! `intro.new`: item 6 the plain at night, 5 the altar's front, 3 / 4 the gargoyle crowds that
//! walk in from the left and the right, 9 the Avatar bound on the altar and 0xc the bonds, 8 /
//! 0xb the gargoyle priest rising with the dagger (0xa once the arrow has struck), 7 the red
//! moongate rising behind the altar, 0 / 2 / 1 the three companions who vault from it and carry
//! the Avatar (9) off. Passages 0xc8 .. 0xdb tell the story, each until a key. The crowds sway:
//! each frame a crowd moves its step with probability 2/3 and turns at ±3 (later ±4).
//!
//! Movements from one set of positions to another go through 0x10f8f2, which steps every
//! coordinate at once along a straight line (Bresenham with one shared length); a key finishes
//! a movement at once.

use crate::engine::Io;

use super::exe::A4;
use super::mem::{self, Ptr};
use super::passages::{passage, show_passage};
use super::screen::{check_key, show, swap};
use super::state::IntroState;

/// Original: A4+0xb3a .. A4+0x2d32 (the movement state of 0x10f8f2)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mover {
    /// Original: A4+0xb3a (word: the next call starts a movement)
    pub start: u16,
    /// Original: A4+0x2d24 / A4+0x2d28 / A4+0x2d2c (allocated arrays: error, distance and
    /// direction of each coordinate)
    pub err: Vec<i16>,
    pub dist: Vec<i16>,
    pub dir: Vec<i16>,
    /// Original: A4+0x2d30 (the longest distance: the number of steps)
    pub len: i16,
    /// Original: A4+0x2d32 (steps left)
    pub left: i16,
}

impl Mover {
    /// The state `u` starts with (A4+0xb3a from the image).
    pub fn new(start: u16) -> Self {
        Self {
            start,
            ..Self::default()
        }
    }

    /// Original: u 0x10f8f2 (move the `n` words of `cur` one step towards `target`; false when
    /// the movement is over)
    ///
    /// The first call of a movement measures each coordinate's distance and direction and sets
    /// every error term to half the longest distance; each call then adds each distance to its
    /// error and steps the coordinate once its error passes the longest distance. After that
    /// many steps the arrays are freed and the next call starts a new movement. A movement with
    /// nothing to move ends at its first call.
    pub fn step(&mut self, cur: &mut [i16], target: &[i16]) -> bool {
        let n = cur.len().min(target.len());
        if self.start != 0 {
            self.err = vec![0; n];
            self.dist = vec![0; n];
            self.dir = vec![0; n];
            self.len = 0;
            for i in 0..n {
                let d = cur[i].wrapping_sub(target[i]);
                let (d, dir) = if d < 0 {
                    (d.wrapping_neg(), 1)
                } else {
                    (d, -1)
                };
                self.dir[i] = dir;
                self.dist[i] = d;
                if d > self.len {
                    self.len = d;
                }
            }
            let half = self.len / 2;
            self.err.iter_mut().for_each(|e| *e = half);
            self.start = 0;
            self.left = self.len;
        }
        let over = self.left == 0;
        self.left = self.left.wrapping_sub(1);
        if over {
            self.err.clear();
            self.dist.clear();
            self.dir.clear();
            self.start = 1;
            return false;
        }
        let len = self.len;
        let steps = self.err.iter_mut().zip(&self.dist).zip(&self.dir);
        for (c, ((e, &d), &dir)) in cur.iter_mut().zip(steps) {
            *e = e.wrapping_add(d);
            if *e > len {
                *e -= len;
                *c = c.wrapping_add(dir);
            }
        }
        true
    }
}

/// Original: A4+0x2b98 .. A4+0x2bc4 (the story's positions)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Story {
    /// Original: A4+0x2b98 (x, y of figure 3, then of figure 4, as they walk in)
    pub figures: [i16; 4],
    /// Original: A4+0x2ba0 (x, y of items 0, 2, 1 and 9 while they fly)
    pub flying: [i16; 8],
    /// Original: A4+0x2bb0 (where they fly to)
    pub flying_to: [i16; 8],
    /// Original: A4+0x2bc0 / A4+0x2bc2 / A4+0x2bc4 (words the story sets and nothing reads; the
    /// last takes a `rand()`)
    pub w2bc0: i16,
    pub w2bc2: i16,
    pub w2bc4: i16,
    pub mover: Mover,
}

/// Original: A4+0x8f8 (4 words: where the figures stop walking)
const FIGURES_TO: u32 = A4 + 0x8f8;

/// Draws item `item` of the story's file on the drawing view.
fn put(st: &mut IntroState, x: i16, y: i16, item: u16) {
    let (v, file): (mem::View, Ptr) = (st.views.draw, st.gypsy.file);
    mem::draw_item(&mut st.video.chip, &v, x, y, file, item);
}

/// Original: u 0x107840 (story: the two figures, 3 at (a − 8, dy + 0x4d) and 4 at (b + 0xb6,
/// dy + 0x4f), on the drawing view reset to the screen view; then a frame wait)
pub async fn figures(st: &mut IntroState, io: &Io, a: i16, b: i16, dy: i16) {
    st.views.draw = st.views.screen;
    put(st, a - 8, dy + 0x4d, 3);
    put(st, b + 0xb6, dy + 0x4f, 4);
    st.video.wait_frame(io).await;
}

/// Original: u 0x1097ac (story: the room view follows the screen view on the shown buffer; then
/// the per-frame key check 0x1039b8)
pub fn poll(st: &mut IntroState, io: &Io) -> u16 {
    st.views.room = st.views.screen;
    st.views.room.planes = st.video.other();
    check_key(st, io)
}

/// Original: u 0x1078da (story: drop the waiting keys through [`poll`])
pub fn drain(st: &mut IntroState, io: &Io) {
    while poll(st, io) != 0 {}
}

/// Original: u 0x1078b6 (story: up to `n` polls, stopping at a key; the delay 0x10fb14 it calls
/// between them is an empty routine of `u`)
pub fn polls(st: &mut IntroState, io: &Io, n: i16) {
    for _ in 0..n.max(0) {
        if poll(st, io) != 0 {
            break;
        }
    }
}

/// The story's key waits: [`poll`] until a key, one frame per poll (the original spins).
async fn until_key(st: &mut IntroState, io: &Io) {
    while poll(st, io) == 0 {
        st.video.wait_frame(io).await;
    }
}

/// A figure's sway: with probability 2/3 the offset moves by its step; at ±`edge` the step
/// turns and moves it back once.
fn sway(st: &mut IntroState, off: &mut i16, step: &mut i16, edge: i16) {
    if st.rand() % 3 != 0 {
        *off += *step;
    }
    if off.abs() == edge {
        *step = -*step;
        *off += *step;
    }
}

/// One movement frame of 0x1078e6: a key finishes the movement at once, else `per_frame` steps
/// (stopping when it ends). Returns (done, Escape).
fn advance(
    st: &mut IntroState,
    io: &Io,
    which: Moving,
    per_frame: i16,
    mut done: bool,
) -> (bool, bool) {
    if poll(st, io) != 0 {
        while move_step(st, which) {}
        done = true;
    }
    if st.skip != 0 {
        return (done, true);
    }
    for _ in 0..per_frame.max(0) {
        if done {
            break;
        }
        done = !move_step(st, which);
    }
    (done, false)
}

/// The coordinate sets the story moves.
#[derive(Clone, Copy)]
enum Moving {
    /// The figures, to A4+0x8f8 (4 words).
    Figures,
    /// The flying items, `n` words of A4+0x2ba0 to A4+0x2bb0.
    Flying(usize),
}

fn move_step(st: &mut IntroState, which: Moving) -> bool {
    let s = &mut st.story;
    match which {
        Moving::Figures => {
            let to = st.exe.a4_words(FIGURES_TO - A4, 4);
            s.mover.step(&mut s.figures, &to)
        }
        Moving::Flying(n) => s.mover.step(&mut s.flying[..n], &s.flying_to[..n]),
    }
}

/// The fixed layers 9, 5 and 0xc of the middle scenes.
fn rest(st: &mut IntroState) {
    put(st, 0x31, 0x44, 9);
    put(st, 0, 0x70, 5);
    put(st, 0xd2, 0x84, 0xc);
}

/// The swaying figures' state (the locals −0xe .. −0x14 of 0x1078e6).
#[derive(Clone, Copy, Default)]
struct Sway {
    a: i16,
    b: i16,
    da: i16,
    db: i16,
}

impl Sway {
    fn step(&mut self, st: &mut IntroState, edge: i16) {
        sway(st, &mut self.a, &mut self.da, edge);
        sway(st, &mut self.b, &mut self.db, edge);
    }
}

/// The figures' bob of the later scenes: 6 − `rand() % 4`.
fn bob(st: &mut IntroState) -> i16 {
    6 - st.rand() % 4
}

/// Fades palette 2 out, clears the screen view, lays passage `n` out on it, swaps, fades back
/// in and waits for a key (0x108474 .. 0x10854c). False on Escape.
async fn page(st: &mut IntroState, io: &Io, n: Option<u16>) -> bool {
    st.video.fade_out(io, st.exe.palette(2)).await;
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    let Some(n) = n else {
        swap(st, io).await;
        st.video.fade_in(io, st.exe.palette(2)).await;
        return true;
    };
    // 0x1031b4: an empty routine of `u`.
    passage(st, io, n);
    show_passage(st);
    swap(st, io).await;
    st.video.fade_in(io, st.exe.palette(2)).await;
    st.views.draw.planes = st.views.screen.planes;
    drain(st, io);
    until_key(st, io).await;
    st.skip == 0
}

/// Original: u 0x1078e6 (introduction story: passages 0xc8 .. 0xdb over the scenes of
/// `intro.new`; Escape ends it)
///
/// Afterwards the screen is cleared, palette 0 set, the menu marked as not drawn and the music
/// stopped. The empty routines of `u` it calls (0x10faf4, 0x1031b6, 0x10fb20, 0x10faf6,
/// 0x10faf2) do nothing.
pub async fn introduction(st: &mut IntroState, io: &Io) {
    st.skip = 0;
    let p = &mut st.views.passage;
    p.y0 = 0;
    p.x0 = 0;
    p.x1 = 0x13f;
    p.y1 = 0x37;
    p.planes = Ptr::at(mem::Region::Work);
    st.views.draw = st.views.screen;
    let (s, d) = (st.views.screen, st.views.draw);
    mem::fill_view(&mut st.video.chip, &s, 0);
    mem::fill_view(&mut st.video.chip, &d, 0);
    body(st, io).await;
    for _ in 0..2 {
        let s = st.views.screen;
        mem::fill_view(&mut st.video.chip, &s, 0);
        swap(st, io).await;
    }
    st.video.set_palette(&st.exe, 0);
    st.skip = 0;
    st.menu_drawn = 0;
    super::music::play_song(st, 0);
}

/// 0x107934..0x109764: the scenes; returns at the end or on Escape.
async fn body(st: &mut IntroState, io: &Io) {
    let quarter: i16 = 0x14;
    st.gypsy.file = st.files_ptr;
    let file = st.files_ptr;
    super::load::load_into(&mut st.video.chip, &st.files, b"intro.new", file);
    st.video.set_palette(&st.exe, 2);
    super::music::play_song(st, 3);
    put(st, 0, 0, 6);
    put(st, 0, 0x70, 5);
    swap(st, io).await;
    st.views.draw.planes = st.views.screen.planes;
    st.story.w2bc2 = 0xc1;
    st.story.w2bc4 = st.rand() % 0xff + 1;
    st.story.w2bc0 = 0;
    st.views.room = st.views.screen;
    st.views.screen.planes = st.video.other();
    st.views.screen.planes = st.video.current();
    // The first two passages go straight onto the shown buffer (the flip does not show).
    passage(st, io, 0xc8);
    polls(st, io, 10);
    st.views.screen.planes = st.video.flip();
    show_passage(st);
    drain(st, io);
    if !key(st, io).await {
        return;
    }
    passage(st, io, 0xc9);
    show_passage(st);
    st.views.screen.planes = st.video.flip();
    drain(st, io);
    if !key(st, io).await {
        return;
    }
    if !walk_in(st, io).await {
        return;
    }
    let mut sw = Sway {
        da: 1,
        db: 1,
        ..Sway::default()
    };
    if !rise(st, io, &mut sw).await {
        return;
    }
    if !pages(st, io, &mut sw, quarter).await {
        return;
    }
    if !fly(st, io, &mut sw).await {
        return;
    }
    end(st, io, &mut sw, quarter).await;
}

/// A key through 0x1039b8, one frame per poll; false on Escape.
async fn key(st: &mut IntroState, io: &Io) -> bool {
    while check_key(st, io) == 0 {
        st.video.wait_frame(io).await;
    }
    st.skip == 0
}

/// [`until_key`], then the Escape check.
async fn next(st: &mut IntroState, io: &Io) -> bool {
    until_key(st, io).await;
    st.skip == 0
}

/// 0x107a34..0x107c42: the figures walk in (passages 0xca .. 0xcc). False on Escape.
async fn walk_in(st: &mut IntroState, io: &Io) -> bool {
    st.story.figures = [-0x54, 0x6d, 0x140, 0xc8];
    st.views.draw = st.views.screen;
    let mut done = false;
    drain(st, io);
    loop {
        let (d, esc) = advance(st, io, Moving::Figures, 1, done);
        done = d;
        if esc {
            return false;
        }
        let f = st.story.figures;
        put(st, 0, 0, 6);
        put(st, f[0], f[1], 3);
        put(st, f[2], f[3], 4);
        put(st, 0, 0x70, 5);
        show(st, io).await;
        if done {
            break;
        }
    }
    passage(st, io, 0xca);
    show_passage(st);
    show(st, io).await;
    put(st, 0, 0, 6);
    figures(st, io, 0, 0, 0).await;
    put(st, 0, 0x70, 5);
    passage(st, io, 0xcb);
    show_passage(st);
    drain(st, io);
    if !next(st, io).await {
        return false;
    }
    show(st, io).await;
    put(st, 0, 0, 6);
    figures(st, io, 0, 0, 0).await;
    rest(st);
    passage(st, io, 0xcc);
    show_passage(st);
    drain(st, io);
    if !next(st, io).await {
        return false;
    }
    show(st, io).await;
    drain(st, io);
    next(st, io).await
}

/// The scene with the risen pair 8 / `top` (0xb or 0xa) and the figures swaying.
async fn risen(st: &mut IntroState, io: &Io, sw: &mut Sway, seven: bool, top: u16, text: bool) {
    put(st, 0, 0, 6);
    if seven {
        put(st, 0x78, 0x3a, 7);
    }
    figures(st, io, sw.a, sw.b, 4).await;
    put(st, 0x90, 0x1e, 8);
    put(st, 0xde, 0x2c, top);
    rest(st);
    if text {
        show_passage(st);
    }
    show(st, io).await;
    sw.step(st, 3);
}

/// [`risen`] with the passage until a key; false on Escape.
async fn risen_until_key(
    st: &mut IntroState,
    io: &Io,
    sw: &mut Sway,
    seven: bool,
    top: u16,
) -> bool {
    loop {
        risen(st, io, sw, seven, top, true).await;
        if poll(st, io) != 0 {
            break;
        }
    }
    st.skip == 0
}

/// The SAS/C clamp of the story's loops (as in the opening).
fn clamp_at(v: i16, to: i16, above: bool) -> i16 {
    if (above && v <= to) || (!above && v >= to) {
        to
    } else {
        v
    }
}

/// 0x107c42..0x108474: 8 and 0xb rise between the figures (0xcd), passages 0xce .. 0xd0, 7
/// comes up (0xd0), then passage 0xd1 alone. False on Escape.
async fn rise(st: &mut IntroState, io: &Io, sw: &mut Sway) -> bool {
    passage(st, io, 0xcd);
    let step: i16 = 2;
    drain(st, io);
    let mut lag: i16 = 0x46;
    let mut y: i16 = 0x8d;
    let mut done = false;
    while !done {
        done = y == 0x1e;
        let mut t = st.rand() % 8 + lag - 3;
        if y > 0x3c || lag == 0 {
            t = lag;
        }
        put(st, 0, 0, 6);
        figures(st, io, 0, 0, 4).await;
        put(st, 0x90 + t, y, 8);
        put(st, 0xde + t, y + 0xe, 0xb);
        rest(st);
        show(st, io).await;
        if lag != 0 {
            lag -= step;
            if lag < 0 {
                lag = 0;
            }
        }
        if poll(st, io) != 0 {
            y = step + 0x1e;
        }
        if st.skip != 0 {
            return false;
        }
        y = clamp_at(y - step, 0x1e, true);
    }
    drain(st, io);
    for n in [None, Some(0xce), Some(0xcf)] {
        if let Some(n) = n {
            passage(st, io, n);
            drain(st, io);
        }
        if !risen_until_key(st, io, sw, false, 0xb).await {
            return false;
        }
    }
    passage(st, io, 0xd0);
    drain(st, io);
    let mut y: i16 = 0x86;
    let mut done = false;
    while !done {
        done = y == 0x3a;
        put(st, 0, 0, 6);
        put(st, 0x78, y, 7);
        figures(st, io, sw.a, sw.b, 4).await;
        put(st, 0x90, 0x1e, 8);
        put(st, 0xde, 0x2c, 0xb);
        rest(st);
        show_passage(st);
        show(st, io).await;
        sw.step(st, 3);
        if poll(st, io) != 0 {
            y = step + 0x3a;
        }
        if st.skip != 0 {
            return false;
        }
        y = clamp_at(y - step, 0x3a, true);
    }
    if !next(st, io).await {
        return false;
    }
    put(st, 0, 0, 6);
    put(st, 0x78, 0x3a, 7);
    figures(st, io, sw.a, sw.b, 4).await;
    put(st, 0x90, 0x1e, 8);
    put(st, 0xde, 0x2c, 0xb);
    rest(st);
    show(st, io).await;
    if st.w25c6 == 0x13 {
        let d = st.views.draw;
        mem::fill_view(&mut st.video.chip, &d, 0);
    }
    for _ in 0..2 {
        let s = st.views.screen;
        mem::fill_view(&mut st.video.chip, &s, 0);
        swap(st, io).await;
    }
    passage(st, io, 0xd1);
    show_passage(st);
    swap(st, io).await;
    st.views.draw.planes = st.views.screen.planes;
    drain(st, io);
    next(st, io).await
}

/// The scene with the figures at rest and the pair 8 / `top`, drawn but not shown.
async fn still(st: &mut IntroState, io: &Io, top: u16) {
    put(st, 0, 0, 6);
    put(st, 0x78, 0x3a, 7);
    figures(st, io, 0, 0, 4).await;
    put(st, 0x90, 0x1e, 8);
    put(st, 0xde, 0x2c, top);
    rest(st);
}

/// 0x108474..0x108bda: three faded pages (0xd2 .. 0xd4), the scene back, 0xb becomes 0xa
/// (0xd5), then 8 / 0xa sink away. False on Escape.
async fn pages(st: &mut IntroState, io: &Io, sw: &mut Sway, quarter: i16) -> bool {
    for n in [0xd2, 0xd3, 0xd4] {
        if !page(st, io, Some(n)).await {
            return false;
        }
    }
    page(st, io, None).await;
    st.views.draw = st.views.screen;
    // 0x1031b4: an empty routine of `u`.
    still(st, io, 0xb).await;
    drain(st, io);
    for _ in 0..quarter / 2 {
        risen(st, io, sw, true, 0xb, false).await;
        if poll(st, io) != 0 {
            break;
        }
        if st.skip != 0 {
            return false;
        }
    }
    still(st, io, 0xa).await;
    for _ in 0..=quarter / 2 {
        show(st, io).await;
    }
    passage(st, io, 0xd5);
    drain(st, io);
    if !risen_until_key(st, io, sw, true, 0xa).await {
        return false;
    }
    let step: i16 = 4;
    drain(st, io);
    let mut y: i16 = 0x1e;
    let mut lag: i16 = 0;
    let mut done = false;
    while !done {
        done = y == 0x8d;
        lag += st.rand() % 9 - 3;
        put(st, 0, 0, 6);
        put(st, 0x78, 0x3a, 7);
        let dy = bob(st);
        figures(st, io, sw.a, sw.b, dy).await;
        put(st, 0x90 + lag, y, 8);
        put(st, 0xde + lag, y + 0xe, 0xa);
        rest(st);
        show(st, io).await;
        sw.step(st, 4);
        if poll(st, io) != 0 {
            y = 0x8d - step;
        }
        if st.skip != 0 {
            return false;
        }
        y = clamp_at(y + step, 0x8d, false);
    }
    true
}

/// One frame of the flying scene: the first `n` / 2 of items 0, 2, 1, 9 at `at`, then 5 (and
/// 0xc while 9 stays in place).
async fn flying(st: &mut IntroState, io: &Io, sw: &mut Sway, at: [i16; 8], n: usize, text: bool) {
    put(st, 0, 0, 6);
    put(st, 0x78, 0x3a, 7);
    let dy = bob(st);
    figures(st, io, sw.a, sw.b, dy).await;
    for (k, item) in [0u16, 2, 1, 9].into_iter().enumerate().take(n / 2) {
        put(st, at[2 * k], at[2 * k + 1], item);
    }
    if n < 8 {
        rest(st);
    } else {
        put(st, 0, 0x70, 5);
    }
    if text {
        show_passage(st);
    }
    show(st, io).await;
    sw.step(st, 4);
}

/// 0x108bda..0x10936a: 0, 2 and 1 fly in (0xd6), then fly off with 9 (0xd7, 0xd8). False on
/// Escape.
async fn fly(st: &mut IntroState, io: &Io, sw: &mut Sway) -> bool {
    let s = &mut st.story;
    s.flying[..6].copy_from_slice(&[-0x20, 0x7a, 0x44, 0x7a, 0xa8, 0xca]);
    s.flying_to[..6].copy_from_slice(&[0x63, 0x28, 0xb7, 0x35, 0xf0, 0x3e]);
    for (n, text) in [(6usize, 0xd6u16), (8, 0xd7)] {
        let per_frame: i16 = 3;
        let mut done = false;
        drain(st, io);
        loop {
            let (d, esc) = advance(st, io, Moving::Flying(n), per_frame, done);
            done = d;
            if esc {
                return false;
            }
            let at = st.story.flying;
            flying(st, io, sw, at, n, false).await;
            if done {
                break;
            }
        }
        passage(st, io, text);
        drain(st, io);
        if n == 6 {
            let at = [0x63, 0x28, 0xb7, 0x35, 0xf0, 0x3e, 0, 0];
            loop {
                flying(st, io, sw, at, 6, true).await;
                if poll(st, io) != 0 {
                    break;
                }
            }
            if st.skip != 0 {
                return false;
            }
            let s = &mut st.story;
            s.flying = [0x63, 0x28, 0xb7, 0x35, 0xf0, 0x3e, 0x31, 0x44];
            s.flying_to = [0x12, 0x8e, 0x12, 0x8e, 0x76, 0x7a, 0x31, 0x8a];
        }
    }
    for n in [None, Some(0xd8)] {
        if let Some(n) = n {
            passage(st, io, n);
            drain(st, io);
        }
        if !alone_until_key(st, io, sw, true).await {
            return false;
        }
    }
    true
}

/// The figures alone (with 7 when `seven`) and the passage until a key; false on Escape.
async fn alone_until_key(st: &mut IntroState, io: &Io, sw: &mut Sway, seven: bool) -> bool {
    loop {
        alone(st, io, sw, seven.then_some(0x3a), true).await;
        if poll(st, io) != 0 {
            break;
        }
    }
    st.skip == 0
}

/// One frame of the figures alone: 6, 7 at (0x78, `y7`) when given, the figures, 5.
async fn alone(st: &mut IntroState, io: &Io, sw: &mut Sway, y7: Option<i16>, text: bool) {
    put(st, 0, 0, 6);
    if let Some(y) = y7 {
        put(st, 0x78, y, 7);
    }
    let dy = bob(st);
    figures(st, io, sw.a, sw.b, dy).await;
    put(st, 0, 0x70, 5);
    if text {
        show_passage(st);
    }
    show(st, io).await;
    sw.step(st, 4);
}

/// 0x10936a..0x109764: 7 sinks (0xd9), the figures alone (0xda, 0xdb), then up to ten more
/// frames.
async fn end(st: &mut IntroState, io: &Io, sw: &mut Sway, quarter: i16) {
    passage(st, io, 0xd9);
    let step: i16 = 1;
    drain(st, io);
    let mut y: i16 = 0x3a;
    let mut done = false;
    while !done {
        done = y == 0x86;
        alone(st, io, sw, Some(y), true).await;
        if poll(st, io) != 0 {
            y = 0x86 - step;
        }
        if st.skip != 0 {
            return;
        }
        y = clamp_at(y + step, 0x86, false);
    }
    for n in [0xda, 0xdb] {
        passage(st, io, n);
        drain(st, io);
        if !alone_until_key(st, io, sw, false).await {
            return;
        }
    }
    drain(st, io);
    for _ in 0..quarter / 2 {
        alone(st, io, sw, None, false).await;
        if poll(st, io) != 0 || st.skip != 0 {
            return;
        }
    }
}

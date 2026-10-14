//! The gypsy's wagon (hunk 4): eight bottles of the virtues, the cards of the questions, and
//! the shimmer above the table as the bottles are poured into the flask.
//!
//! Virtues by index: 0 Honesty, 1 Compassion, 2 Valor, 3 Justice, 4 Sacrifice, 5 Honor,
//! 6 Spirituality, 7 Humility (the order of the question table A4+0x426 and of the answer
//! tables). Every table the drawing reads is a word table of `u`'s data hunk, 8 entries per
//! virtue unless noted; the port reads them through [`UExe`](super::exe::UExe) as it draws.

use crate::engine::Io;

use super::mem::{self, Ptr, Region, View};
use super::passages::show_passage;
use super::screen::swap;
use super::state::IntroState;

/// Original: A4+0x2aec / A4+0x2b1c / A4+0x2b2c / A4+0x2b4c and the shimmer globals
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gypsy {
    /// Original: A4+0x2aec (word per virtue: liquid left in its bottle, 3 = full)
    pub level: [i16; 8],
    /// Original: A4+0x2b1c (the virtues still in the contest, in drawing order)
    pub order: [i16; 8],
    /// Original: A4+0x2b2c (16 bytes: the colours the shimmer picks from)
    pub shimmer_colours: [u8; 16],
    /// Original: A4+0x2b4c (word per pour: the colour it added)
    pub poured: [i16; 8],
    /// Original: A4+0x2b1a (pours so far)
    pub pours: i16,
    /// Original: A4+0x2afc (word: the shimmer runs)
    pub shimmer_on: i16,
    /// Original: A4+0x402 (word: the stream from the bottle to the flask is drawn)
    pub stream: i16,
    /// Original: A4+0x2ae8 (the scene's `.new` file: `woods.new`, then `gypsy.new`)
    pub file: Ptr,
}

/// Word `i` of the table at `A4 + off`.
fn t(st: &IntroState, off: u32, i: i32) -> i16 {
    st.exe.a4_i16(off.wrapping_add_signed(2 * i))
}

/// The scene's vertical offset (A4+0x414, −20 for the wagon).
fn y_off(st: &IntroState) -> i16 {
    st.exe.a4_i16(0x414)
}

fn draw(st: &mut IntroState, x: i16, y: i16, item: i16) {
    let v = st.views.draw;
    let file = st.gypsy.file;
    mem::draw_item(&mut st.video.chip, &v, x, y, file, item as u16);
}

/// Original: u 0x1059a8 (gypsy: one more pour of `colour`: the shimmer starts 4 lines higher
/// and its 16 colours are shared out among the pours so far)
pub fn add_pour(st: &mut IntroState, colour: i16) {
    st.views.shimmer.y0 -= 4;
    let g = &mut st.gypsy;
    if let Some(slot) = g.poured.get_mut(g.pours as usize) {
        *slot = colour;
    }
    g.pours += 1;
    for k in 1..=g.pours {
        let n = 16 / g.pours;
        for j in 0..n {
            if let Some(c) = g.shimmer_colours.get_mut((j * k) as usize) {
                *c = g.poured[(k - 1) as usize] as u8;
            }
        }
    }
}

/// Original: u 0x119944 (gypsy: eight shimmer pixels: noise nibbles through the colour table,
/// as five plane bytes)
pub fn shimmer_byte(st: &mut IntroState) -> [u8; 5] {
    let mut planes = [0u8; 5];
    for _ in 0..8 {
        let n = st.noise_word() & 0xf;
        let c = st.gypsy.shimmer_colours[usize::from(n)];
        for (p, b) in planes.iter_mut().enumerate() {
            *b = (*b << 1) | ((c >> p) & 1);
        }
    }
    planes
}

/// Original: u 0x105a04 (gypsy: the shimmer: noise rows into the work bitmap, then copied to
/// the screen's rectangle A4+0x2b0a)
pub fn shimmer(st: &mut IntroState) {
    let work = Ptr::at(Region::Work);
    let View {
        x1: right,
        x0: left,
        y0: top,
        y1: bottom,
        ..
    } = st.views.shimmer;
    let mut y = top as u16;
    while y <= bottom as u16 {
        let mut xb = (left / 8) as u16;
        while xb <= (right / 8) as u16 {
            let planes = shimmer_byte(st);
            let at = usize::from(y) * mem::ROW + usize::from(xb);
            let bytes = st.video.chip.bytes_mut(work);
            for (p, &b) in planes.iter().enumerate() {
                if let Some(slot) = bytes.get_mut(p * mem::PLANE + at) {
                    *slot = b;
                }
            }
            xb += 1;
        }
        y += 1;
    }
    let src = View {
        planes: work,
        ..st.views.shimmer
    };
    st.views.shimmer.planes = st.views.screen.planes;
    let dst = st.views.shimmer;
    mem::copy_view(&mut st.video.chip, &src, &dst);
}

/// Original: u 0x105aea (gypsy: the liquid in bottle `v` for the bottle item `item` at (x, y))
pub fn draw_liquid(st: &mut IntroState, v: i16, item: i16, x: i16, y: i16) {
    let kind = match item {
        9 | 17 => 0,
        10 | 18 => 1,
        16 | 46 => 2,
        11..=14 => 3,
        _ => return,
    };
    let lvl = st.gypsy.level[v as usize] - 1;
    let base = if v > 5 && kind != 3 {
        st.exe.a4_i16(0x548)
    } else {
        st.exe.a4_i16(0x54a)
    };
    if lvl <= -1 || lvl >= 3 {
        return;
    }
    st.views.draw.planes = st.views.screen.planes;
    let (v, lvl) = (i32::from(v), i32::from(lvl));
    let x = x + t(st, 0x60c, kind * 0x18 + lvl * 8 + v) + t(st, 0x404, v);
    let y = y + t(st, 0x6cc, kind * 3 + lvl) + y_off(st);
    let item = base + t(st, 0x54c, kind * 0x18 + lvl * 8 + v);
    draw(st, x, y, item);
}

/// Original: u 0x105c24 (gypsy: frame `frame` of bottle `v` pouring, the other bottles, the
/// flask and the stream; `again` = 1 when the frame is drawn a second time) — returns false
/// when the pour is over for this bottle's level (nothing drawn)
///
/// Frames 0..16 (16 is 0 again) lift, tilt and lower the bottle; at frame 11 of a first
/// drawing its level drops. The bottle's level decides on which frame the pour ends and when
/// the stream shows.
pub fn draw_bottles(st: &mut IntroState, v: i16, frame: i16, again: i16) -> bool {
    let base_y: i16 = 0x56;
    st.views.draw.planes = st.views.screen.planes;
    st.gypsy.stream = 0;
    let f = frame % 16;
    let vi = i32::from(v);
    let mut done = false;
    let lift = st.exe.a4_i16(0x546);
    let lifted_x = t(st, 0x404, vi) + t(st, 0x704, vi) - 6;
    let lifted_y = base_y + y_off(st) - 0x2d;
    let tilted = |st: &IntroState| (t(st, 0x6f4, vi), 0x29 + y_off(st));
    let level = |st: &IntroState| st.gypsy.level[v as usize];
    let (mut x, mut y, item);
    match f {
        0 => {
            x = t(st, 0x704, vi);
            y = base_y;
            item = t(st, 0x7a4, vi);
        }
        1 | 15 => {
            draw(st, lifted_x, lifted_y, lift + t(st, 0x814, vi));
            x = t(st, 0x704, vi);
            y = base_y;
            item = t(st, 0x7b4, vi);
        }
        2 | 14 => {
            draw(st, lifted_x, lifted_y, lift + t(st, 0x824, vi));
            x = t(st, 0x704, vi);
            y = base_y - 0x25;
            item = t(st, 0x7b4, vi);
        }
        3 | 13 => {
            draw(st, lifted_x, lifted_y, lift + t(st, 0x824, vi));
            x = t(st, 0x714, vi);
            y = base_y - 0x29;
            item = t(st, 0x7c4, vi);
        }
        4 | 12 => {
            let (tx, ty) = tilted(st);
            draw(st, tx, ty, lift + t(st, 0x834, vi));
            x = t(st, 0x724, vi);
            y = t(st, 0x764, vi);
            item = t(st, 0x7d4, vi);
        }
        5 | 11 => {
            let (tx, ty) = tilted(st);
            draw(st, tx, ty, lift + t(st, 0x834, vi));
            x = t(st, 0x734, vi);
            y = t(st, 0x774, vi);
            item = t(st, 0x7e4, vi);
            if f == 11 && again == 0 {
                st.gypsy.level[v as usize] -= 1;
            }
            if level(st) == 2 && f == 11 {
                done = true;
            }
            if level(st) == 3 {
                st.gypsy.stream = 1;
            }
        }
        6 | 10 => {
            let (tx, ty) = tilted(st);
            draw(st, tx, ty, lift + t(st, 0x834, vi));
            x = t(st, 0x744, vi);
            y = t(st, 0x784, vi);
            item = t(st, 0x7f4, vi);
            if (level(st) == 1 || level(st) == 2) && f == 10 {
                done = true;
            }
            if level(st) == 2 {
                st.gypsy.stream = 1;
            }
            if level(st) == 3 {
                done = true;
            }
        }
        7 | 9 => {
            let (tx, ty) = tilted(st);
            draw(st, tx, ty, lift + t(st, 0x834, vi));
            x = t(st, 0x754, vi);
            y = t(st, 0x794, vi);
            item = t(st, 0x804, vi);
            if level(st) == 1 && f == 9 {
                done = true;
            }
            if level(st) == 2 || level(st) == 3 {
                done = true;
            }
        }
        _ => {
            let (tx, ty) = tilted(st);
            draw(st, tx, ty, lift + t(st, 0x834, vi));
            x = t(st, 0x754, vi);
            y = t(st, 0x794, vi);
            item = t(st, 0x804, vi);
            if level(st) == 1 {
                st.gypsy.stream = 1;
            }
            if level(st) == 2 || level(st) == 3 {
                done = true;
            }
        }
    }
    if (4..=12).contains(&f) {
        x -= t(st, 0x404, vi);
    }
    if done {
        return false;
    }
    draw_liquid(st, v, item, x, y);
    let dy = y_off(st);
    y += dy;
    draw(st, t(st, 0x404, vi) + x, y, st.exe.a4_i16(0x548) + item);
    for d in 0..8i32 {
        if d == vi {
            continue;
        }
        let lvl = st.gypsy.level[d as usize];
        let bx = t(st, 0x404, d) + t(st, 0x704, d);
        if lvl > 0 {
            let item = st.exe.a4_i16(0x54a) + t(st, 0x6e4, d) + lvl - 3;
            draw(st, bx + 3, 0x61 + dy, item);
        }
        let item = st.exe.a4_i16(0x548) + t(st, 0x7a4, d);
        draw(st, bx, base_y + dy, item);
    }
    if f < 9 && st.gypsy.stream != 0 && again == 0 {
        let colour = t(st, 0x416, vi);
        add_pour(st, colour);
        st.gypsy.shimmer_on += 1;
    }
    if st.gypsy.shimmer_on != 0 {
        shimmer(st);
    }
    draw(st, 0x8e, 0x4c + dy, st.exe.a4_i16(0x548) + 7);
    if st.gypsy.stream != 0 {
        let lvl = i32::from(3 - level(st));
        let sx = t(st, 0x874, lvl * 8 + vi);
        let sy = t(st, 0x8a4, lvl) + dy;
        let base = if v > 5 {
            st.exe.a4_i16(0x548)
        } else {
            st.exe.a4_i16(0x54a)
        };
        draw(st, sx, sy, base + t(st, 0x844, lvl * 8 + vi));
    }
    true
}

/// Original: u 0x106312 (gypsy: the pointer's save view follows the screen; 0x10fb42 and
/// 0x10fb36 are empty)
pub fn pointer_view(st: &mut IntroState) {
    st.views.v2ac8.planes = st.views.screen.planes;
}

/// Original: u 0x10632a (gypsy: shuffle the first `n` entries of the contest order with
/// `n + rand % n²` random swaps)
pub fn shuffle(st: &mut IntroState, n: i16) {
    let extra = st.random(n.wrapping_mul(n));
    let mut swaps = n.wrapping_add(extra);
    while swaps != 0 {
        let a = st.random(n) as usize;
        let b = st.random(n) as usize;
        st.gypsy.order.swap(a, b);
        swaps -= 1;
    }
}

/// The gypsy scene as it stands (background with the passage, then the bottles at rest) into
/// the drawing buffer.
fn compose(st: &mut IntroState) {
    st.views.draw.planes = st.views.screen.planes;
    let (src, dst) = (st.views.gypsy_bg, st.views.screen);
    mem::copy_view(&mut st.video.chip, &src, &dst);
}

/// The passage drawn onto the saved background (0x10f4da with the screen view on it).
fn passage_on_background(st: &mut IntroState) {
    st.views.draw.planes = st.views.screen.planes;
    st.views.panel.planes = st.views.screen.planes;
    let keep = st.views.screen.planes;
    st.views.screen.planes = st.views.gypsy_bg.planes;
    show_passage(st);
    st.views.screen.planes = keep;
}

/// Original: u 0x106386 (gypsy: show the scene until a key; `pointer` = keep the pointer view
/// following) — returns the key, upper-cased
///
/// The passage is drawn onto the saved background once; then each frame the background and the
/// bottles at rest are composed and swapped in, the first frame also copied into the other
/// buffer. A mouse click on the A / B buttons would answer too (the port has no mouse).
pub async fn wait_answer(st: &mut IntroState, io: &Io, pointer: bool) -> u16 {
    passage_on_background(st);
    let mut first = true;
    loop {
        compose(st);
        draw_bottles(st, 0, 0, 1);
        if pointer {
            pointer_view(st);
        }
        swap(st, io).await;
        if first {
            first = false;
            let (src, dst) = (st.views.draw, st.views.screen);
            mem::copy_view(&mut st.video.chip, &src, &dst);
        }
        if st.keys.ready(io) {
            let c = st.keys.take(io);
            return super::input::to_upper(c);
        }
    }
}

/// Original: u 0x1064c0 (gypsy: ask question `p` until A or B; returns the virtue chosen,
/// 0..7)
///
/// With A4+0x182e set the A and B buttons (items A4+0x546 + 6 / + 7) are drawn into the
/// passage box first.
pub async fn ask(st: &mut IntroState, io: &Io, p: i16) -> i16 {
    if st.buttons_on != 0 {
        st.views.draw.planes = st.views.screen.planes;
        let v = st.views.passage;
        let base = st.exe.a4_i16(0x546);
        let file = st.gypsy.file;
        mem::draw_item(&mut st.video.chip, &v, 0x10f, 0x29, file, (base + 6) as u16);
        mem::draw_item(&mut st.video.chip, &v, 0x120, 0x29, file, (base + 7) as u16);
    }
    let c = loop {
        let c = wait_answer(st, io, true).await;
        if c == u16::from(b'A') || c == u16::from(b'B') {
            break c;
        }
    };
    let table: u32 = if c == u16::from(b'A') { 0x3d4 } else { 0x40c };
    // The original indexes its per-virtue tables with the word as it is, sign-extended
    // (0x106530), so a negative question word reads before the table; the port does the same
    // 32-bit address sum. A `u` whose tables hold one past 7 would read beyond them: the port
    // keeps the low three bits (0..7 are unchanged).
    st.exe.a4_i16(table.wrapping_add_signed(2 * i32::from(p))) & 7
}

/// Original: u 0x106554 (gypsy: virtue `v` won a question: add its stats, Humility's karma,
/// then pour its bottle)
///
/// Strength, dexterity and intelligence grow by the tables A4+0x516 / 0x526 / 0x536. The pour
/// runs frames 0..16 of [`draw_bottles`]; each frame that draws is shown, then drawn again
/// twice (the same frame, `again` = 1) at one swap each.
pub async fn pour(st: &mut IntroState, io: &Io, v: i16) {
    let vi = u32::from(v as u16);
    st.hero.str += st.exe.a4_i16(0x516 + 2 * vi);
    st.hero.dex += st.exe.a4_i16(0x526 + 2 * vi);
    st.hero.int += st.exe.a4_i16(0x536 + 2 * vi);
    if v == 7 {
        st.hero.karma = st.hero.karma.wrapping_add(3);
    }
    let repeats = 2;
    st.views.draw.planes = st.views.screen.planes;
    st.views.panel.planes = st.views.screen.planes;
    let (src, dst) = (st.views.gypsy_bg, st.views.screen);
    mem::copy_view(&mut st.video.chip, &src, &dst);
    passage_on_background(st);
    for frame in 0..0x11 {
        if !draw_bottles(st, v, frame, 0) {
            continue;
        }
        pointer_view(st);
        swap(st, io).await;
        for _ in 0..repeats {
            compose(st);
            draw_bottles(st, v, frame, 1);
            pointer_view(st);
            swap(st, io).await;
        }
        compose(st);
    }
}

/// The question passage of the pair (a, b): the word at A4+0x426 + 16 a + 2 b.
pub fn question(st: &IntroState, a: i16, b: i16) -> i16 {
    st.exe
        .a4_i16(0x426 + (16 * i32::from(a) + 2 * i32::from(b)) as u32)
}

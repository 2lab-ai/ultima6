//! The storm of the opening (0x1041b0 from 0x1046c2 on): clouds, rain and lightning seen
//! through the window, the bolt that strikes the earth, then the room pans to the open front
//! door and the view goes out into the forest (`intro_2.new`, `intro_2a.new`).
//!
//! `intro_2.new` holds the scene outside: item 1 the sky, 0 the cloud bank that scrolls in two
//! copies, 2 / 3 the clouds, 4..7 the rain drops, 8 the trees, 0xa the ground and the path,
//! 0xb..0x12 the lightning bolts, 0x13..0x17 the bolt that strikes (at (0x9e, 0x72)), 0x18 /
//! 0x19 the wall left and right of the open front door, 0x1a / 0x1b the scene's light, dark or
//! lit by the lightning. `intro_2a.new` is a whole screen in the composite bitmap: the room with
//! its window, overlaid on the scene (0x11653c) and scrolled away to the left while the door
//! comes in (0x116490).
//!
//! The clouds (0x103c6c / 0x103d32) and the rain (0x103e80 / 0x103ea4) keep their state in
//! globals; both draw through the view A4+0x17e4 (the pointers A4+0x2692 and A4+0x2abe always
//! name it), which the port reads as `views.draw`.

use crate::engine::Io;

use super::load;
use super::mem::{self, Ptr, Region};
use super::passages::{passage, show_passage};
use super::screen::{check_key, show};
use super::state::IntroState;

/// Rain drops the arrays hold (A4+0x269e .. A4+0x2ab8: 150 words × 3, then 150 bytes).
pub const DROPS: usize = 150;
/// Clouds the arrays hold (A4+0x2652 .. A4+0x268e: 10 words × 3).
pub const CLOUDS: usize = 10;

/// Original: A4+0x2652 .. A4+0x269c (the clouds of 0x103c6c / 0x103d32)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clouds {
    /// Original: A4+0x2696 (clouds in use)
    pub count: i16,
    /// Original: A4+0x2652 / A4+0x2666 / A4+0x267a (x, y and item of each cloud)
    pub x: [i16; CLOUDS],
    pub y: [i16; CLOUDS],
    pub item: [i16; CLOUDS],
    /// Original: A4+0x268e / A4+0x2690 (position of the cloud bank, item 0)
    pub bank_x: i16,
    pub bank_y: i16,
    /// Original: A4+0x269c (pixels a cloud moves each frame; the bank moves half)
    pub speed: i16,
    /// Original: A4+0x2698 (the `.new` file the clouds come from)
    pub file: Ptr,
}

/// Original: A4+0x269e .. A4+0x2ac4 (the rain of 0x103e80 / 0x103ea4)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rain {
    /// Original: A4+0x2ab8 (drops at most, less one)
    pub max: i16,
    /// Original: A4+0x2aba (drops falling)
    pub count: i16,
    /// Original: A4+0x2abc (frames a drop waits between moves)
    pub delay: i16,
    /// Original: A4+0x2ac2 (a new drop starts when `rand() % chance` is 0)
    pub chance: i16,
    /// Original: A4+0x269e / A4+0x27ca / A4+0x28f6 / A4+0x2a22 (x, y, wait and item of each
    /// drop)
    pub x: [i16; DROPS],
    pub y: [i16; DROPS],
    pub wait: [i16; DROPS],
    pub item: [u8; DROPS],
    /// Original: A4+0x2ac4 (the `.new` file the drops come from)
    pub file: Ptr,
}

impl Default for Rain {
    fn default() -> Self {
        Self {
            max: 0,
            count: 0,
            delay: 0,
            chance: 0,
            x: [0; DROPS],
            y: [0; DROPS],
            wait: [0; DROPS],
            item: [0; DROPS],
            file: Ptr::default(),
        }
    }
}

/// The storm's globals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Storm {
    pub clouds: Clouds,
    pub rain: Rain,
}

/// Original: u 0x103c38 (opening: a random x within the view, x0 ..= x1)
pub fn random_x(st: &mut IntroState, v: &mem::View) -> i16 {
    st.random(v.x1 - v.x0 + 1) + v.x0
}

/// Original: u 0x103c52 (opening: a random y within the view, y0 ..= y1)
pub fn random_y(st: &mut IntroState, v: &mem::View) -> i16 {
    st.random(v.y1 - v.y0 + 1) + v.y0
}

/// A new cloud `i` left of the view: x in x0 .. x1 less `back` and 0x8c, y in 0 .. 29, item 2
/// or 3 (the rand calls in the original's order).
fn place_cloud(st: &mut IntroState, i: usize, back: i16) {
    let v = st.views.draw;
    let x = st.random(v.x1 - v.x0) + v.x0 - back;
    let y = st.random(0x1e);
    let item = (st.rand() & 1) + 2;
    let c = &mut st.storm.clouds;
    c.x[i] = x - 0x8c;
    c.y[i] = y;
    c.item[i] = item;
}

/// Original: u 0x103c6c (opening: start `n` clouds of `file` left of the view; the bank at
/// (−400, −5))
pub fn clouds_init(st: &mut IntroState, n: i16, file: Ptr) {
    st.storm.clouds.file = file;
    st.storm.clouds.count = n;
    for i in 0..n.clamp(0, CLOUDS as i16) as usize {
        place_cloud(st, i, 0x104);
    }
    st.storm.clouds.bank_x = -400;
    st.storm.clouds.bank_y = -5;
}

/// Original: u 0x103d32 (opening: draw and move the clouds, then the cloud bank twice, 320
/// pixels apart)
///
/// A cloud past the view's right edge starts again at the left. The bank moves half the clouds'
/// speed and wraps at 320.
pub fn clouds_step(st: &mut IntroState) {
    let v = st.views.draw;
    let file = st.storm.clouds.file;
    for i in 0..st.storm.clouds.count.clamp(0, CLOUDS as i16) as usize {
        let c = &st.storm.clouds;
        let (x, y, item) = (c.x[i], c.y[i], c.item[i]);
        mem::draw_item(&mut st.video.chip, &v, x, y, file, item as u16);
        if st.storm.clouds.x[i] > v.x1 {
            place_cloud(st, i, 0x140);
        }
        let c = &mut st.storm.clouds;
        c.x[i] = c.x[i].wrapping_add(c.speed);
    }
    let c = &st.storm.clouds;
    let (bx, by) = (c.bank_x, c.bank_y);
    mem::draw_item(&mut st.video.chip, &v, bx, by, file, 0);
    mem::draw_item(&mut st.video.chip, &v, bx - 0x140, by, file, 0);
    let c = &mut st.storm.clouds;
    c.bank_x += c.speed >> 1;
    if c.bank_x == 0x140 {
        c.bank_x = 0;
    }
}

/// Original: u 0x103e80 (opening: start the rain: at most `max` drops (less one), each waiting
/// `delay` frames between moves, none falling yet)
pub fn rain_init(st: &mut IntroState, max: i16, delay: i16) {
    let r = &mut st.storm.rain;
    r.max = max - 1;
    r.count = 0;
    r.delay = delay;
}

/// A drop's x and y at the top of the view (rand calls in the original's order: the caller
/// picks which comes first).
fn drop_x(st: &mut IntroState) -> i16 {
    let v = st.views.draw;
    st.random(v.x1 - v.x0 + 0x15) + (v.x0 - 0xa)
}

fn drop_y(st: &mut IntroState) -> i16 {
    let v = st.views.draw;
    st.random(9) + v.y0 - 4
}

/// Original: u 0x103ea4 (opening: the rain: maybe one more drop, then every drop moves (2, 10)
/// when its wait is over and is drawn; a drop below the view starts again at the top)
///
/// A new drop starts when there is room and `rand() % chance` is 0: item 4..7, x from 10 left
/// of the view to 10 right of it, y up to 4 above the top. It is drawn at once and again in the
/// loop.
pub fn rain_step(st: &mut IntroState) {
    let v = st.views.draw;
    let file = st.storm.rain.file;
    let (count, max, chance) = (st.storm.rain.count, st.storm.rain.max, st.storm.rain.chance);
    if count != max && st.random(chance) == 0 {
        let n = st.storm.rain.count.clamp(0, DROPS as i16 - 1) as usize;
        let item = ((st.rand() & 3) + 4) as u8;
        let x = drop_x(st);
        let y = drop_y(st);
        let r = &mut st.storm.rain;
        r.item[n] = item;
        r.x[n] = x;
        r.y[n] = y;
        r.wait[n] = r.delay;
        mem::draw_item(&mut st.video.chip, &v, x, y, file, u16::from(item));
        st.storm.rain.count += 1;
    }
    for i in 0..st.storm.rain.count.clamp(0, DROPS as i16) as usize {
        let r = &mut st.storm.rain;
        if r.wait[i] != 0 {
            r.wait[i] -= 1;
        } else {
            r.wait[i] = r.delay;
            r.x[i] = r.x[i].wrapping_add(2);
            r.y[i] = r.y[i].wrapping_add(10);
            if r.y[i] > v.y1 {
                let y = drop_y(st);
                let x = drop_x(st);
                let item = ((st.rand() & 3) + 4) as u8;
                let r = &mut st.storm.rain;
                r.y[i] = y;
                r.x[i] = x;
                r.item[i] = item;
            }
        }
        let r = &st.storm.rain;
        let (x, y, item) = (r.x[i], r.y[i], r.item[i]);
        mem::draw_item(&mut st.video.chip, &v, x, y, file, u16::from(item));
    }
}

/// Draws item `item` of the scene file on the drawing view at (x, y).
fn put(st: &mut IntroState, file: Ptr, x: i16, y: i16, item: u16) {
    let v = st.views.draw;
    mem::draw_item(&mut st.video.chip, &v, x, y, file, item);
}

/// The storm's phases (the local at −0xc of 0x1041b0).
mod phase {
    /// Rain and lightning, no passage yet (40 frames or a key).
    pub const RAIN: i16 = 0;
    /// Passages 3 and 4 (a key each).
    pub const TEXT: i16 = 1;
    /// Passage 5: the bolt strikes, the scene stays lit.
    pub const STRIKE: i16 = 3;
    /// Passage 6: the room scrolls left, the open front door comes in.
    pub const PAN: i16 = 4;
    /// The door.
    pub const DOOR: i16 = 5;
    /// The walls move apart: out through the door.
    pub const OUT: i16 = 6;
    /// Passage 8 in the forest.
    pub const FOREST: i16 = 7;
}

/// 0x1046c2..0x104b1c of 0x1041b0: the storm. True when it ran to its end, false when Escape
/// jumped to the end of the opening (0x10558e).
///
/// After the last TV room passage a key is awaited with the storm loaded. Each frame: sky,
/// clouds, a lightning bolt now and then (when `rand() % 6` is 0 after 60 frames, 1..4 frames
/// long, jittered by ±1), ground, trees, the striking bolt (a different item each frame) while
/// it strikes, the rain, the scene's light (lit while the lightning flickers or the bolt
/// strikes), then the phase's own layer: the room, the room scrolling away with the door coming
/// in, the door, the walls moving apart. Keys step the phases; passage 3 is laid out at the
/// start, 4, 5, 6 and 8 by the keys that leave phases 1, 2, 3 and 6 (phase 6 also lays out 8
/// when its walls are apart).
pub(super) async fn storm(st: &mut IntroState, io: &Io) -> bool {
    let file = st.files_ptr;
    st.storm.rain.file = file;
    load::load_into(&mut st.video.chip, &st.files, b"intro_2.new", file);
    let comp = Ptr::at(Region::Composite);
    load::load_into(&mut st.video.chip, &st.files, b"intro_2a.new", comp);
    mem::composite_mask(&mut st.video.chip);
    while check_key(st, io) == 0 {
        st.video.wait_frame(io).await;
    }
    if st.skip != 0 {
        return false;
    }
    st.video.queued_palette = 9;
    let r = &mut st.views.room;
    r.x1 = 0x101;
    r.x0 = 0x39;
    r.y0 = 0;
    r.y1 = 0xb0;
    st.views.pan.y0 = 0;
    st.views.pan.y1 = 0x2e;
    let pan_step: i16 = 8;
    st.storm.clouds.speed = 4;
    st.storm.rain.chance = 2;
    let part_step: i16 = 2;
    rain_init(st, 0x96, 0);
    clouds_init(st, 5, file);
    passage(st, io, 3);
    let mut frame: i16 = 0;
    let mut done = false;
    let mut x: i16 = 0;
    let mut flash: i16 = 0;
    let (mut bolt, mut bolt_x, mut bolt_y) = (0i16, 0i16, 0i16);
    let mut drop: i16 = 0;
    let mut ph = phase::RAIN;
    let mut shown: i16 = 0;
    while !done {
        put(st, file, 0, 0, 1);
        clouds_step(st);
        if st.random(6) == 0 && frame >= 0x3c && flash == 0 && shown != 0 {
            flash = (st.rand() & 3) + 1;
            bolt = st.rand() & 7;
            let pan = st.views.pan;
            bolt_x = random_x(st, &pan);
            bolt_y = random_y(st, &pan);
        }
        if flash != 0 {
            let bx = st.random(3) + bolt_x - 2;
            let by = st.random(3) + bolt_y - 2;
            put(st, file, bx, by, (0xb + bolt) as u16);
        }
        put(st, file, 0, 0x4c, 0xa);
        put(st, file, 0, 0, 8);
        let last = drop;
        while drop == last {
            drop = st.random(5) + 8;
        }
        if ph == phase::STRIKE || (ph == phase::PAN && x < 0xa0) {
            put(st, file, 0x9e, 0x72, (0xb + drop) as u16);
        }
        rain_step(st);
        if ph < phase::DOOR {
            let lit = (flash != 0 && st.rand() & 1 != 0) || ph == phase::STRIKE;
            let (rx, ry) = (st.views.room.x0 - x, st.views.room.y0);
            put(st, file, rx, ry, if lit { 0x1b } else { 0x1a });
        }
        match ph {
            phase::RAIN => mem::overlay_composite(&mut st.video.chip, st.video.draw),
            1..=phase::STRIKE => {
                mem::overlay_composite(&mut st.video.chip, st.video.draw);
                show_passage(st);
            }
            phase::PAN => {
                mem::overlay_composite_left(&mut st.video.chip, st.video.draw, x);
                put(st, file, 0x140 - x, 0, 0x18);
                put(st, file, 0x140 - x + 0xfd, 0, 0x19);
                x += pan_step;
                if x > 0x140 {
                    ph = phase::DOOR;
                    x = 0;
                }
                show_passage(st);
            }
            phase::DOOR => {
                put(st, file, 0, 0, 0x18);
                put(st, file, 0xfd, 0, 0x19);
                show_passage(st);
            }
            phase::OUT => {
                put(st, file, -x, 0, 0x18);
                put(st, file, 0xfd + x, 0, 0x19);
                x += part_step;
                if x > 0x4b {
                    passage(st, io, 8);
                    x = 0;
                    ph = phase::FOREST;
                }
            }
            phase::FOREST => show_passage(st),
            _ => {}
        }
        if shown == 0 {
            shown += 1;
        }
        show(st, io).await;
        if check_key(st, io) != 0 {
            match ph {
                phase::RAIN | phase::PAN => {
                    ph += 1;
                    x = 0;
                }
                1..=phase::STRIKE => {
                    passage(st, io, (ph + 3) as u16);
                    ph += 1;
                }
                phase::DOOR => ph += 1,
                phase::OUT => {
                    passage(st, io, 8);
                    ph += 1;
                    x = 0;
                }
                _ => done = true,
            }
        }
        if st.skip != 0 {
            return false;
        }
        if flash != 0 {
            flash -= 1;
        }
        frame = if frame == 1000 { 900 } else { frame + 1 };
        if frame >= 0x28 && ph == phase::RAIN {
            ph = phase::TEXT;
        }
    }
    true
}

//! The opening of `u`: ORIGIN, Lord British, the TV room (0x1041b0 and its helpers in hunk 2);
//! the storm and the stones follow in [`super::storm`], the moongate and the portal in
//! [`super::gate`].
//!
//! `intro_1a.new` and `intro_1b.new` are whole 5-plane screens loaded straight into the drawing
//! buffer and faded in and out with palette 0. `intro_1.new` holds the TV room: item 0 the room,
//! 0xd the front layer, 0xe the TV's glow, 0xf / 1 the pan layers, 2..0xc the VCR clock digits
//! and 0x10 + n the TV picture frames. The TV runs one of seven byte scripts (A4+0x1c6, 0x50
//! bytes each; a digit key switches script) through 0x103a42, frame positions from the tables
//! A4+0xe6 / A4+0x156. Passages 0, 1 and 2 are the story text, each shown until a key.

use crate::engine::Io;

use super::exe::A4;
use super::mem::{self, Ptr, Region, View};
use super::passages::{passage, show_passage};
use super::screen::{check_key, show, swap};
use super::state::IntroState;

/// TV frames the position tables cover (A4+0xe6 and A4+0x156, 56 words each).
const TV_FRAMES: usize = 56;

/// The TV of the room.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tv {
    /// Original: A4+0x263a (script number 0..6)
    pub script: i16,
    /// Original: A4+0x263c (position in the script)
    pub pos: i16,
    /// Original: A4+0x3fa (frame drawn; −1 = none)
    pub frame: i16,
    /// Original: A4+0x25f2 / A4+0x25f0 (repeat count and loop start of command 0x80)
    pub repeat: i16,
    pub loop_at: i16,
    /// Original: A4+0x3f8 (the last random pick of command 0x84)
    pub last_pick: i16,
    /// Original: A4+0x3fc (flicker counter of command 0x83)
    pub flicker: i16,
    /// Original: A4+0x3f6 (blink counter of command 0x88)
    pub blink: i16,
    /// Original: A4+0x25f4 (the frame command 0x87 picked)
    pub picked: i16,
    /// Original: A4+0x2636 (word: static was drawn this step)
    pub static_drawn: u16,
    /// Original: A4+0x2638 (left shift of the picture while the TV leaves the screen)
    pub shift: i16,
    /// Original: A4+0xe6 / A4+0x156 (x / y offset of each frame; command 0x83 rewrites the y of
    /// frame 0x22)
    pub dx: Vec<i16>,
    pub dy: Vec<i16>,
    /// Original: A4+0x263e (the file the frames come from, `intro_1.new`)
    pub file: Ptr,
}

impl Tv {
    pub fn new(st: &IntroState) -> Self {
        let e = &st.exe;
        Self {
            frame: e.a4_i16(0x3fa),
            last_pick: e.a4_i16(0x3f8),
            flicker: e.a4_i16(0x3fc),
            blink: e.a4_i16(0x3f6),
            dx: e.a4_words(0xe6, TV_FRAMES),
            dy: e.a4_words(0x156, TV_FRAMES),
            file: Ptr::at(Region::Files),
            ..Self::default()
        }
    }
}

/// Original: u 0x103956 (opening: start TV script `n` from its first byte)
pub fn set_script(st: &mut IntroState, n: i16) {
    st.tv.pos = 0;
    st.tv.script = n;
}

/// The byte at position `at` of the TV script. The original adds the script's 0x50-byte row
/// (`muls.w`) and the sign-extended position word (`adda.w`, 0x103a60) as a 32-bit address, so
/// a position that wrapped negative reads below the script; the port makes the same sum.
fn script_byte(st: &mut IntroState, at: i16) -> u8 {
    let row = i32::from(st.tv.script) * 0x50;
    let addr = (A4 + 0x1c6).wrapping_add_signed(row + i32::from(at));
    st.exe.byte(addr)
}

fn next_byte(st: &mut IntroState) -> u8 {
    let pos = st.tv.pos;
    st.tv.pos = pos.wrapping_add(1);
    script_byte(st, pos)
}

/// Original: u 0x103966 (opening: draw TV frame `n`: item n + 0x10 at the TV view's corner plus
/// the frame's offsets, less the shift)
pub fn draw_frame(st: &mut IntroState, n: i16) {
    st.views.tv.planes = st.views.screen.planes;
    let i = n as usize;
    let x = st.views.tv.x0 + st.tv.dx.get(i).copied().unwrap_or(0) - st.tv.shift;
    let y = st.views.tv.y0 + st.tv.dy.get(i).copied().unwrap_or(0);
    let v = st.views.tv;
    mem::draw_item(&mut st.video.chip, &v, x, y, st.tv.file, (n + 0x10) as u16);
}

/// Original: u 0x103830 (opening: TV static: random bytes in planes 0..3 of the TV rectangle of
/// the work bitmap, plane 4 clear, copied to the screen)
pub fn draw_static(st: &mut IntroState) {
    let work = Ptr::at(Region::Work);
    let tv = st.views.tv;
    for y in 0x32..=0x56i32 {
        for xb in (0xe5 / 8)..=(0x11d / 8) {
            let b = st.rand() as u8;
            let at = (y * 40 + xb) as usize;
            let bytes = st.video.chip.bytes_mut(work);
            for p in 0..4 {
                bytes[p * mem::PLANE + at] = b;
            }
            bytes[4 * mem::PLANE + at] = 0;
        }
    }
    let src = View { planes: work, ..tv };
    mem::copy_view(&mut st.video.chip, &src, &st.views.tv.clone());
    st.tv.static_drawn = 1;
}

/// Original: u 0x103a42 (opening: run the TV script up to the end of this step)
///
/// Bytes below 0x80 are frames to draw; 0x80 n starts a loop of n (n ≥ 10 halves it), 0x81
/// repeats it and ends the step, 0x82 draws static and ends, 0x83 flickers frame 0x22 twice,
/// 0x84 n draws one of the next n frames at random (never the same twice; for n ≤ 1 its one
/// draw), 0x85 goes to the next script, 0x86 / 0x87 draw / pick a frame from A4+0xd4, 0x88
/// blinks frames 0x25 / 0x26, 0x89 draws a random one of 0x32..0x34, 0x8a ends the step, 0x8b
/// nothing. The last frame number stays in A4+0x3fa and is drawn again by commands that do not
/// set it.
pub fn tv_step(st: &mut IntroState) {
    st.tv.static_drawn = 0;
    for _ in 0..4096 {
        let b = next_byte(st);
        let mut done = false;
        if b < 0x80 {
            st.tv.frame = i16::from(b);
        } else {
            match b {
                0x80 => {
                    let n = i16::from(next_byte(st));
                    st.tv.repeat = if n >= 10 { n >> 1 } else { n };
                    st.tv.loop_at = st.tv.pos;
                }
                0x81 => {
                    if st.tv.repeat != 0 {
                        st.tv.repeat -= 1;
                        st.tv.pos = st.tv.loop_at;
                    }
                    done = true;
                }
                0x82 => {
                    draw_static(st);
                    st.tv.frame = -1;
                    done = true;
                }
                0x83 => {
                    let f = st.tv.flicker;
                    st.tv.flicker -= 1;
                    if f == 0 {
                        st.tv.flicker = 0xe;
                    }
                    st.tv.dy[0x22] = 0x15 - st.tv.flicker;
                    draw_frame(st, 0x22);
                    st.tv.dy[0x22] = 0x24 - st.tv.flicker;
                    draw_frame(st, 0x22);
                    st.tv.frame = 0x21;
                }
                0x84 => {
                    let n = i16::from(next_byte(st));
                    // The original draws again while the pick repeats the last one (0x103b94);
                    // with one frame or none to pick from it can only repeat and loops for ever.
                    // The port keeps that one draw. The pick and the step are word sums (`add.w`
                    // at 0x103b8a / 0x103bae): a position past 0x7fff wraps negative.
                    let pick = loop {
                        let p = st.tv.pos.wrapping_add(st.random(n));
                        if st.tv.last_pick != p || n <= 1 {
                            break p;
                        }
                    };
                    st.tv.last_pick = pick;
                    let f = i16::from(script_byte(st, pick));
                    st.tv.pos = st.tv.pos.wrapping_add(n);
                    st.tv.frame = f;
                }
                0x85 => {
                    st.tv.pos = 0;
                    st.tv.script = (st.tv.script + 1) % 7;
                }
                0x86 => st.tv.frame = st.tv.picked,
                0x87 => {
                    let i = st.random(9);
                    st.tv.picked = st.exe.a4_i16(0xd4 + 2 * i as u32);
                }
                0x88 => {
                    st.tv.blink = (st.tv.blink + 1) & 0xff;
                    if st.tv.blink & 4 != 0 {
                        st.tv.frame = ((st.tv.blink & 0x10) >> 4) + 0x25;
                    }
                }
                0x89 => st.tv.frame = st.random(3) + 0x32,
                0x8a => done = true,
                _ => {}
            }
        }
        if done {
            return;
        }
        if st.tv.frame > -1 {
            let f = st.tv.frame;
            draw_frame(st, f);
        }
    }
}

/// Original: u 0x1040aa (opening: the VCR clock, hours and minutes of the local time, digits of
/// `file` at (x, 20) .. (x + 14, 20))
pub fn draw_clock(st: &mut IntroState, x: i16, view: View, file: Ptr) {
    let (hour, minute) = super::seams::local_time();
    let h = st.exe.word(0x0010_56d0 + 2 * u32::from(hour % 24)) as i16;
    let chip = &mut st.video.chip;
    let first = if h < 10 { 0xc } else { h / 10 + 2 };
    mem::draw_item(chip, &view, x, 0x14, file, first as u16);
    mem::draw_item(chip, &view, x + 4, 0x14, file, (h % 10 + 2) as u16);
    let m = minute as i16;
    mem::draw_item(chip, &view, x + 0xa, 0x14, file, (m / 10 + 2) as u16);
    mem::draw_item(chip, &view, x + 0xe, 0x14, file, (m % 10 + 2) as u16);
}

/// One frame of the TV room: room, front layer, TV, glow after static, then the clock.
fn room_frame(st: &mut IntroState, with_passage: bool) {
    let file = st.tv.file;
    let (room, front) = (st.views.room, st.views.front);
    mem::draw_item(&mut st.video.chip, &room, 0, 0, file, 0);
    mem::draw_item(&mut st.video.chip, &front, 0, 0x3f, file, 0xd);
    tv_step(st);
    if st.tv.static_drawn != 0 {
        let front = st.views.front;
        mem::draw_item(&mut st.video.chip, &front, 0x8f, 0x5b, file, 0xe);
        st.tv.static_drawn = 0;
    }
    if with_passage {
        show_passage(st);
    }
    let draw = st.views.draw;
    draw_clock(st, 0xdd, draw, file);
}

/// Original: u 0x1041b0 (opening: ORIGIN, Lord British, the TV room, the storm and the stones,
/// the moongate and the gargoyles' portal; Escape skips to the end)
///
/// From `intro_2.new` on (0x1046c2..0x10558e) the body continues in [`super::storm::storm`]
/// and [`super::gate::gate`].
pub async fn opening(st: &mut IntroState, io: &Io) {
    let v = &mut st.views;
    v.front = v.draw;
    v.pan = v.front;
    v.room = v.pan;
    v.passage = View {
        planes: Ptr::at(Region::Work),
        ..View::screen(v.screen.planes)
    };
    v.passage.y1 = 0x1f;
    check_key(st, io);
    if st.skip == 0 && body(st, io).await {
        let (s, d) = (st.views.screen, st.views.draw);
        mem::fill_view(&mut st.video.chip, &s, 0);
        mem::fill_view(&mut st.video.chip, &d, 0);
    }
    // 0x1055a2: two black swaps, colour cycling off, palette 0, music off.
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    swap(st, io).await;
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    swap(st, io).await;
    st.video.cycling = 0;
    st.video.set_palette(&st.exe, 0);
    super::music::play_song(st, 0);
}

/// Waits `n` frames polling the keys; true when Escape was pressed.
async fn hold(st: &mut IntroState, io: &Io, n: u16) -> bool {
    for _ in 0..n {
        check_key(st, io);
        if st.skip != 0 {
            return true;
        }
        st.video.wait_frame(io).await;
    }
    false
}

/// The TV room loop until a key (passage shown); true when Escape was pressed.
async fn room_until_key(st: &mut IntroState, io: &Io) -> bool {
    loop {
        room_frame(st, true);
        show(st, io).await;
        if check_key(st, io) != 0 {
            return st.skip != 0;
        }
    }
}

/// 0x1041b0 from the first music call to 0x10558e; true = ended through 0x10558e (every path
/// but the Escape check before the first picture).
async fn body(st: &mut IntroState, io: &Io) -> bool {
    super::music::play_song(st, 2);
    st.tv.file = st.files_ptr;
    let file = st.tv.file;
    super::load::load_into(&mut st.video.chip, &st.files, b"intro_1.new", file);
    let draw = Ptr::at(st.video.draw);
    super::load::load_into(&mut st.video.chip, &st.files, b"intro_1a.new", draw);
    super::music::start_music(st);
    swap(st, io).await;
    st.video.fade_in(io, st.exe.palette(0)).await;
    st.views.draw.planes = st.views.screen.planes;
    if hold(st, io, 0x32).await {
        return true;
    }
    let draw = Ptr::at(st.video.draw);
    super::load::load_into(&mut st.video.chip, &st.files, b"intro_1b.new", draw);
    st.video.fade_out(io, st.exe.palette(0)).await;
    swap(st, io).await;
    st.video.fade_in(io, st.exe.palette(0)).await;
    let p = st.views.screen.planes;
    for v in [
        &mut st.views.draw,
        &mut st.views.front,
        &mut st.views.pan,
        &mut st.views.room,
    ] {
        v.planes = p;
    }
    if hold(st, io, 0x32).await {
        return true;
    }
    st.video.fade_out(io, st.exe.palette(0)).await;
    let (s, d) = (st.views.screen, st.views.draw);
    mem::fill_view(&mut st.video.chip, &s, 0);
    mem::fill_view(&mut st.video.chip, &d, 0);
    check_key(st, io);
    if st.skip != 0 {
        return true;
    }
    st.views.tv = View {
        x0: 0xe5,
        y0: 0x32,
        x1: 0x11d,
        y1: 0x56,
        ..st.views.draw
    };
    set_script(st, 2);
    passage(st, io, 0);
    {
        let (room, front) = (st.views.room, st.views.front);
        mem::draw_item(&mut st.video.chip, &room, 0, 0, file, 0);
        mem::draw_item(&mut st.video.chip, &front, 0, 0x3f, file, 0xd);
        let draw = st.views.draw;
        draw_clock(st, 0xdd, draw, file);
    }
    swap(st, io).await;
    st.video.fade_in(io, st.exe.palette(0)).await;
    let p = st.views.screen.planes;
    for v in [&mut st.views.draw, &mut st.views.front, &mut st.views.room] {
        v.planes = p;
    }
    check_key(st, io);
    if st.skip != 0 {
        return true;
    }
    st.tv.shift = 0;
    for _ in 0..0x1e {
        room_frame(st, false);
        show(st, io).await;
        if check_key(st, io) != 0 {
            break;
        }
    }
    if st.skip != 0 || room_until_key(st, io).await {
        return true;
    }
    st.keys.flush(io);
    passage(st, io, 1);
    if room_until_key(st, io).await {
        return true;
    }
    passage(st, io, 2);
    if pan_away(st, io).await {
        return true;
    }
    if super::storm::storm(st, io).await {
        super::gate::gate(st, io).await;
    }
    true
}

/// 0x104572..0x1046c2: the room pans left by 4 pixels a frame while the last passage shows;
/// a key jumps to the end. True when Escape was pressed.
async fn pan_away(st: &mut IntroState, io: &Io) -> bool {
    let step = 4;
    let file = st.tv.file;
    let mut x: i16 = 0;
    let mut done = false;
    while !done {
        done = x == 0x140;
        let v = st.views.clone();
        let chip = &mut st.video.chip;
        if x >= 0x87 {
            let px = ((x - 0x87) * 10) / 0x24 + (0x140 - x) + 0x4d;
            mem::draw_item(chip, &v.pan, px, 0, file, 0xf);
        }
        mem::draw_item(chip, &v.room, v.room.x0 - x, 0, file, 0);
        mem::draw_item(chip, &v.pan, 0x140 + v.pan.x0 - x, 0, file, 1);
        mem::draw_item(chip, &v.front, v.front.x0 - 2 * x, 0x3f, file, 0xd);
        show_passage(st);
        if st.views.tv.x1 > 0 && x < 0x12c {
            tv_step(st);
        }
        st.views.tv.x1 -= step;
        st.views.tv.x0 -= step;
        if st.views.tv.x0 < 0 {
            st.tv.shift -= st.views.tv.x0;
            st.views.tv.x0 = 0;
        }
        let draw = st.views.draw;
        draw_clock(st, 0xdd - x, draw, file);
        show(st, io).await;
        if check_key(st, io) != 0 {
            x = 0x140 - step;
        }
        if st.skip != 0 {
            return true;
        }
        x += step;
        if x >= 0x140 {
            x = 0x140;
        }
    }
    false
}

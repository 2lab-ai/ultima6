//! Character creation of `u` (0x106666 and its helpers in hunks 1, 4 and 8): name and sex on
//! the vellum, the portrait, the walk through the woods, the gypsy's seven questions, the flask
//! and the farewell, then the new character into the save's objlist (0x105700).

use crate::engine::Io;

use super::gypsy::{self, ask, pour, question, shuffle, wait_answer};
use super::input::to_upper;
use super::mem::{self, Ptr, Region, View};
use super::passages::{passage, show_passage};
use super::screen::{poll_key, swap, swap_gradient};
use super::state::IntroState;
use super::text;

/// Strings of the name screen (image addresses in `u`).
const ASK_NAME: u32 = 0x0010_57f0;
const ECHO_NAME: u32 = 0x0010_5818;
const ASK_SEX: u32 = 0x0010_5820;
const NAME_FMT: u32 = 0x0010_9a14;
const MALE: u32 = 0x0010_99a2;
const FEMALE: u32 = 0x0010_99a8;
/// Original: A4+0x9a2 (the keys of the portrait buttons: sex, portrait, accept, abort, Escape)
const PORTRAIT_KEYS: u32 = super::exe::A4 + 0x9a2;

fn ctype(st: &IntroState, c: u16) -> u8 {
    st.keys
        .ctype
        .get(usize::from(c & 0xff))
        .copied()
        .unwrap_or(0)
}

fn put(st: &mut IntroState, c: u8) {
    if let Some(font) = st.font.as_ref() {
        text::put_char(&mut st.video.chip, &mut st.text, font, c);
    }
}

fn print_new(st: &mut IntroState, fmt: &[u8], args: &[text::Arg]) {
    if let Some(font) = st.font.as_ref() {
        text::print_new(&mut st.video.chip, &mut st.text, font, fmt, args);
    }
}

fn with_font(
    st: &mut IntroState,
    f: impl FnOnce(&mut mem::Chip, &mut text::TextCtx, &u6_formats::font::WouFont),
) {
    if let Some(font) = st.font.as_ref() {
        f(&mut st.video.chip, &mut st.text, font);
    }
}

/// Original: u 0x102e4a (input: wait for a key with a blinking `_` at the cursor)
///
/// Every 20 frames the underscore is drawn in ink or in paper. Accepted: Backspace, Return,
/// Escape, space, letters, digits and punctuation (C type bits 0..2 and 4); an extended code
/// (0 then a scan code) gives Backspace for 0x4b and 0x15 for 0x4d. The underscore is erased
/// before returning.
pub async fn get_key(st: &mut IntroState, io: &Io) -> u16 {
    let (x, y) = (st.text.x, st.text.y);
    let ink = st.text.fg;
    let mut blink = 0;
    let mut c;
    loop {
        st.video.wait_frame(io).await;
        blink += 1;
        if blink == 0x14 {
            blink = 0;
            st.text.x = x;
            st.text.y = y;
            put(st, b'_');
            st.text.fg = if st.text.fg == ink { st.text.bg } else { ink };
        }
        if !st.keys.ready(io) {
            continue;
        }
        c = st.keys.take(io);
        let mut done = false;
        if c == 0 {
            c = match st.keys.take(io) {
                0x4b => {
                    done = true;
                    8
                }
                0x4d => {
                    done = true;
                    0x15
                }
                _ => 0,
            };
        } else if matches!(c, 8 | 0x0d | 0x1b | 0x20) {
            done = true;
        }
        let t = ctype(st, c);
        if done || t & 7 != 0 || t & 0x10 != 0 {
            break;
        }
    }
    st.text.x = x;
    st.text.y = y;
    st.text.fg = st.text.bg;
    put(st, b'_');
    st.text.fg = ink;
    st.text.x = x;
    st.text.y = y;
    c
}

/// Original: u 0x102f66 (input: a line of at most `max` characters; `None` on Escape or Return
/// on an empty line)
///
/// Letters, digits, space and `- ' . ,` are taken (and drawn, except for one-character input,
/// which returns as soon as it has its character); Backspace erases the last one in the paper
/// colour.
pub async fn read_line(st: &mut IntroState, io: &Io, max: i16) -> Option<Vec<u8>> {
    let mut chars: Vec<(u8, i16, i16)> = Vec::new();
    loop {
        let c = if max == 1 && !chars.is_empty() {
            0x0d
        } else {
            get_key(st, io).await & 0xff
        };
        if c == 0x1b || (c == 0x0d && chars.is_empty()) {
            return None;
        }
        if c == 8
            && let Some(&(ch, x, y)) = chars.last()
        {
            chars.pop();
            st.text.x = x;
            st.text.y = y;
            let ink = st.text.fg;
            st.text.fg = st.text.bg;
            put(st, ch);
            st.text.fg = ink;
            st.text.x = x;
            st.text.y = y;
        }
        if c == 0x0d {
            return Some(chars.iter().map(|&(ch, _, _)| ch).collect());
        }
        let ok = ctype(st, c) & 7 != 0 || matches!(c as u8, b' ' | b'-' | b'\'' | b'.' | b',');
        if ok && (chars.len() as i16) < max {
            chars.push((c as u8, st.text.x, st.text.y));
            if max != 1 {
                put(st, c as u8);
            }
        }
    }
}

/// Original: u 0x109e44 (creation: switch the sex answer: 'F' → "Male", 'M' → "Female")
///
/// Called twice after the answer it spells the answer out ("M" → "Female" → "Male").
pub fn toggle_sex(st: &mut IntroState) {
    let name = match st.hero.sex_char() {
        b'F' => MALE,
        b'M' => FEMALE,
        _ => return,
    };
    st.hero.sex = st.exe.cstr(name);
}

/// Original: u 0x105876 (creation: ask name and sex on the vellum) — false on Escape at the
/// name
pub async fn name_and_sex(st: &mut IntroState, io: &Io) -> bool {
    let mut box_ = View {
        planes: st.views.screen.planes,
        ..st.views.portrait
    };
    mem::fill_view(&mut st.video.chip, &box_, 0x1e);
    st.text.fg = 0;
    st.text.bg = 0x1e;
    box_.x1 -= 16;
    box_.x0 += 16;
    box_.y0 += 16;
    box_.y1 -= 16;
    st.text.view = box_;
    with_font(st, text::clear_view);
    st.text.align = 2;
    let ask = st.exe.cstr(ASK_NAME);
    print_new(st, &ask, &[]);
    loop {
        let y = st.text.y;
        st.text.x = box_.x0;
        st.text.y = y;
        with_font(st, |c, t, f| text::clear_eos(c, t, f));
        st.text.x = 0x35;
        let Some(name) = read_line(st, io, 0xd).await else {
            return false;
        };
        let n = name.len().min(13);
        st.hero.name[..n].copy_from_slice(&name[..n]);
        st.hero.name[n] = 0;
        st.text.x = box_.x0;
        with_font(st, |c, t, f| text::clear_eol(c, t, f));
        st.text.align = 2;
        let (echo, sex_q) = (st.exe.cstr(ECHO_NAME), st.exe.cstr(ASK_SEX));
        print_new(st, &echo, &[text::Arg::Str(st.hero.name_str())]);
        print_new(st, &sex_q, &[]);
        let answered = loop {
            st.text.x = 0x9d;
            with_font(st, |c, t, f| text::clear_eol(c, t, f));
            let r = read_line(st, io, 1).await;
            if let Some(s) = &r {
                st.hero.sex = s.clone();
            }
            let c = to_upper(u16::from(st.hero.sex_char())) as u8;
            if let Some(first) = st.hero.sex.first_mut() {
                *first = c;
            }
            if c == b'M' || c == b'F' || r.is_none() {
                break r.is_some();
            }
        };
        if answered {
            toggle_sex(st);
            toggle_sex(st);
            return true;
        }
    }
}

/// Original: u 0x1030f8 (input: wait for one of `keys` (upper-cased) or a click on its button)
/// — returns its index
///
/// The original spins; the port waits a frame between polls. The buttons need a mouse, which
/// the port has not.
pub async fn choose(st: &mut IntroState, io: &Io, keys: &[u8]) -> usize {
    loop {
        if st.keys.ready(io) {
            let c = to_upper(st.keys.take(io));
            if let Some(i) = keys.iter().position(|&k| u16::from(k) == c) {
                return i;
            }
            continue;
        }
        st.video.wait_frame(io).await;
    }
}

/// Original: u 0x10a15e (creation / transfer: choose the portrait; S switches sex, P the next
/// of six portraits, C (T when transferring) accepts, A or Escape aborts) — true on accept
pub async fn choose_portrait(st: &mut IntroState, io: &Io, transfer: bool) -> bool {
    let mut keys = st.exe.bytes(PORTRAIT_KEYS, 5);
    let accept_item: u16 = if transfer {
        keys[2] = b'T';
        1
    } else {
        keys[2] = b'C';
        0x10
    };
    let planes = st.views.screen.planes;
    st.views.portrait.planes = planes;
    let mut pic = View {
        planes,
        x1: 0x102,
        x0: 0xc3,
        y0: 0x76,
        y1: 0xad,
    };
    let rect = |x1, x0, y0, y1| View {
        planes,
        x1,
        x0,
        y0,
        y1,
    };
    let b = &mut st.views.buttons;
    b[0] = rect(0x123, 0x1e, 0x57, 0x63);
    b[1] = rect(0x123, 0x1e, 0x63, 0xae);
    b[2] = rect(0x97, 0x5e, 0x70, 0x7f);
    b[3] = rect(0x97, 0x3e, 0x81, 0x90);
    b[4] = rect(0x97, 0x56, 0x92, 0xa1);
    b[5] = rect(0x97, 0x50, 0xa3, 0xb2);
    b[6] = b[5];
    st.views.draw.planes = planes;
    let portrait = st.views.portrait;
    mem::fill_view(&mut st.video.chip, &portrait, 0x1e);
    let mut strip = st.views.buttons[1];
    strip.y0 -= 1;
    strip.y1 = strip.y0;
    mem::fill_view(&mut st.video.chip, &strip, 0x1e);
    st.text.fg = 0;
    st.text.bg = 0x1e;
    st.text.view = st.views.buttons[0];
    st.text.x = st.views.buttons[0].x0;
    st.text.y = st.views.buttons[0].y0 + 2;
    st.text.align = 2;
    let fmt = st.exe.cstr(NAME_FMT);
    print_new(st, &fmt, &[text::Arg::Str(st.hero.name_str())]);
    let vellum = st.files_ptr;
    let b = st.views.buttons;
    for (k, item) in [(2usize, 3u16), (3, 0x12), (4, accept_item), (5, 4)] {
        mem::draw_item(
            &mut st.video.chip,
            &portrait,
            b[k].x0,
            b[k].y0,
            vellum,
            item,
        );
    }
    let montage = st.files_ptr.add(0x9c40);
    loop {
        st.video.pointer = 0;
        pic.planes = st.views.screen.planes;
        mem::fill_view(&mut st.video.chip, &pic, 0x1e);
        let base = if st.hero.sex_char() == b'F' { 0 } else { 6 };
        st.hero.portrait_item = st.hero.portrait + base;
        let item = st.hero.portrait_item as u16;
        mem::draw_item(&mut st.video.chip, &pic, pic.x0, pic.y0, montage, item);
        st.video.pointer = 1;
        let i = choose(st, io, &keys).await;
        match keys.get(i).copied().unwrap_or(0x1b) {
            0x1b | b'A' => return false,
            b'C' | b'T' => return true,
            b'P' => st.hero.portrait = (st.hero.portrait + 1) % 6,
            b'S' => toggle_sex(st),
            _ => {}
        }
    }
}

/// Draws woods items 0 and 1 scrolled by `x` and swaps (one frame of the walk).
async fn woods_frame(st: &mut IntroState, io: &Io, x: i16, with_passage: bool) {
    st.views.draw.planes = st.views.screen.planes;
    let (v, file) = (st.views.draw, st.gypsy.file);
    mem::draw_item(&mut st.video.chip, &v, v.x0 - x, 0, file, 0);
    if x != 0 {
        mem::draw_item(&mut st.video.chip, &v, 0x140 + v.x0 - x, 0, file, 1);
    }
    if with_passage {
        show_passage(st);
    }
    swap_gradient(st, io).await;
}

/// A key wait in the original's busy-loop form (0x1039d2 until nonzero), one frame per poll.
async fn until_key(st: &mut IntroState, io: &Io) {
    while poll_key(st, io) == 0 {
        st.video.wait_frame(io).await;
    }
}

/// One frame of a full-screen gypsy item with the passage, swapped through the gradient lists.
async fn gypsy_page(st: &mut IntroState, io: &Io, item: u16, with_passage: bool) {
    st.views.draw.planes = st.views.screen.planes;
    let (v, file) = (st.views.draw, st.gypsy.file);
    mem::draw_item(&mut st.video.chip, &v, 0, 0, file, item);
    if with_passage {
        show_passage(st);
    }
    swap_gradient(st, io).await;
}

/// Original: u 0x106666 (creation: the whole sequence) — 0 = created, 1 = cancelled or a game
/// already in progress
pub async fn create(st: &mut IntroState, io: &Io) -> u16 {
    st.views.draw.planes = st.video.other();
    let (d, s) = (st.views.draw, st.views.screen);
    mem::copy_view(&mut st.video.chip, &d, &s);
    let p = st.video.flip();
    st.views.screen.planes = p;
    st.views.draw.planes = p;
    st.views.panel.planes = p;
    let v = &mut st.views;
    v.v2ac8 = v.screen;
    v.portrait = View {
        x1: 0x124,
        x0: 0x1c,
        y0: 0x57,
        y1: 0xbd,
        ..v.draw
    };
    v.v2c64 = View {
        x1: 0x124,
        x0: 0x1c,
        y0: 0x57,
        y1: 0xbd,
        ..v.screen
    };
    v.v17f4 = v.panel;
    v.v17f4.planes = v.draw.planes;
    v.passage = View {
        planes: v.screen.planes,
        x1: 0x13f,
        x0: 0,
        y0: 0,
        y1: 0x37,
    };
    let saved = View {
        planes: Ptr::at(Region::Work),
        x1: v.panel.x1 - v.panel.x0,
        x0: 0,
        y0: 0,
        y1: v.panel.y1 - v.panel.y0,
    };
    let vellum = st.files_ptr;
    super::load::load_into(&mut st.video.chip, &st.files, b"vellum1.new", vellum);
    let montage = st.files_ptr.add(0x9c40);
    super::load::load_into(&mut st.video.chip, &st.files, b"montage.new", montage);
    let panel = st.views.panel;
    mem::copy_view(&mut st.video.chip, &panel, &saved);
    st.views.panel.planes = st.views.screen.planes;
    st.views.v17f4.planes = st.views.screen.planes;
    let panel = st.views.panel;
    mem::fill_view(&mut st.video.chip, &panel, 0);
    st.video.set_palette(&st.exe, 6);
    let v = st.views.v17f4;
    mem::draw_item(&mut st.video.chip, &v, v.x0, v.y0, vellum, 0);
    st.hero.portrait = 0;
    st.video.pointer = 1;
    super::seams::play_song(st, 4);
    st.menu_drawn = 0;
    loop {
        st.video.pointer = 0;
        if !name_and_sex(st, io).await {
            st.menu_drawn = 1;
            break;
        }
        st.video.pointer = 1;
        if choose_portrait(st, io, false).await {
            break;
        }
    }
    if st.menu_drawn != 0 {
        st.views.panel.planes = st.views.screen.planes;
        let panel = st.views.panel;
        mem::fill_view(&mut st.video.chip, &panel, 0);
        mem::copy_view(&mut st.video.chip, &saved, &panel);
        swap(st, io).await;
        super::seams::play_song(st, 0);
        return 1;
    }
    st.video.pointer = 0;
    woods(st, io).await;
    wagon(st, io).await;
    farewell(st, io).await;
    finish(st, io).await
}

/// 0x10685a..0x106a92: the walk through the woods to the wagon (passages 0x64..0x66).
async fn woods(st: &mut IntroState, io: &Io) {
    st.gypsy.file = st.files_ptr;
    let file = st.gypsy.file;
    super::load::load_into(&mut st.video.chip, &st.files, b"woods.new", file);
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    swap(st, io).await;
    st.video.set_palette(&st.exe, 3);
    woods_frame(st, io, 0, false).await;
    st.views.passage.planes = Ptr::at(Region::Work);
    passage(st, io, 0x64);
    loop {
        woods_frame(st, io, 0, true).await;
        if poll_key(st, io) != 0 {
            break;
        }
    }
    let step = 2;
    let mut x: i16 = 0;
    let mut done = false;
    while !done {
        done = x == 0xbf;
        st.views.draw.planes = st.views.screen.planes;
        let (v, file) = (st.views.draw, st.gypsy.file);
        mem::draw_item(&mut st.video.chip, &v, v.x0 - x, 0, file, 0);
        mem::draw_item(&mut st.video.chip, &v, 0x140 + v.x0 - x, 0, file, 1);
        swap_gradient(st, io).await;
        if poll_key(st, io) != 0 {
            x = 0xbf - step;
        }
        x += step;
        if x >= 0xbf {
            x = 0xbf;
        }
    }
    for n in [0x65, 0x66] {
        passage(st, io, n);
        loop {
            woods_frame(st, io, 0xbf, true).await;
            if poll_key(st, io) != 0 {
                break;
            }
        }
    }
    woods_frame(st, io, 0xbf, false).await;
    st.menu_drawn = 0;
}

/// 0x106a98..0x106cd0: the wagon: the bottles, then three rounds of questions (4, 2, 1).
async fn wagon(st: &mut IntroState, io: &Io) {
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    swap(st, io).await;
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    st.video.set_palette(&st.exe, 4);
    st.gypsy.file = st.files_ptr;
    let file = st.gypsy.file;
    super::load::load_into(&mut st.video.chip, &st.files, b"gypsy.new", file);
    st.views.draw.planes = st.views.screen.planes;
    st.hero.str = 0xf;
    st.hero.dex = 0xf;
    st.hero.int = 0xf;
    for i in 0..8 {
        st.gypsy.order[i] = i as i16;
        st.gypsy.level[i] = 3;
        st.gypsy.shimmer_colours[i] = 0xff;
    }
    st.gypsy.shimmer_on = 0;
    st.gypsy.pours = 0;
    let top = 0x7f + st.exe.a4_i16(0x414);
    st.views.shimmer = View {
        planes: st.views.draw.planes,
        x1: 0xac,
        x0: 0x93,
        y0: top,
        y1: top,
    };
    let (v, y) = (st.views.draw, st.exe.a4_i16(0x414));
    mem::draw_item(&mut st.video.chip, &v, 0, y, file, 0);
    st.views.gypsy_bg = View {
        planes: st.files_ptr.add(0x3_30cc),
        ..st.views.screen
    };
    let (s, bg) = (st.views.screen, st.views.gypsy_bg);
    mem::copy_view(&mut st.video.chip, &s, &bg);
    gypsy::draw_bottles(st, 0, 0, 1);
    swap(st, io).await;
    for n in [0x67, 0x68] {
        passage(st, io, n);
        wait_answer(st, io, false).await;
    }
    st.video.pointer = 1;
    shuffle(st, 8);
    for i in 0..4 {
        let w = round(st, io, i).await;
        st.gypsy.order[i] = w;
    }
    shuffle(st, 4);
    for i in 0..2 {
        let w = round(st, io, i).await;
        st.gypsy.order[i] = w;
    }
    round(st, io, 0).await;
}

/// One question between the pair at `2 i`, `2 i + 1` of the contest order; returns the winner.
async fn round(st: &mut IntroState, io: &Io, i: usize) -> i16 {
    let (a, b) = (st.gypsy.order[2 * i], st.gypsy.order[2 * i + 1]);
    let p = question(st, a, b);
    passage(st, io, p as u16);
    let w = ask(st, io, p).await;
    pour(st, io, w).await;
    w
}

/// 0x106cd0..0x106f08: the flask, the gypsy's words, the farewell pages.
async fn farewell(st: &mut IntroState, io: &Io) {
    st.video.pointer = 0;
    passage(st, io, 0x85);
    wait_answer(st, io, false).await;
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    swap(st, io).await;
    st.video.set_palette(&st.exe, 8);
    let sh = &mut st.views.shimmer;
    (sh.x1, sh.x0, sh.y0, sh.y1) = (0xcb, 0x74, 0x23, 0x7f);
    gypsy::shimmer(st);
    st.views.draw.planes = st.views.screen.planes;
    let (v, file) = (st.views.draw, st.gypsy.file);
    mem::draw_item(&mut st.video.chip, &v, 0, 0, file, 0xc6);
    passage(st, io, 0x86);
    swap(st, io).await;
    loop {
        st.views.draw.planes = st.views.screen.planes;
        gypsy::shimmer(st);
        let (v, file) = (st.views.draw, st.gypsy.file);
        mem::draw_item(&mut st.video.chip, &v, 0, 0, file, 0xc6);
        show_passage(st);
        swap(st, io).await;
        if poll_key(st, io) != 0 {
            break;
        }
    }
    gypsy::shimmer(st);
    st.views.draw.planes = st.views.screen.planes;
    let (v, file) = (st.views.draw, st.gypsy.file);
    mem::draw_item(&mut st.video.chip, &v, 0, 0, file, 0xc6);
    swap(st, io).await;
    if st.w25c6 == 0x13 {
        let s = st.views.screen;
        mem::fill_view(&mut st.video.chip, &s, 0x75);
    }
    st.video.fade_out(io, st.exe.palette(8)).await;
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    swap(st, io).await;
    st.views.draw.planes = st.views.screen.planes;
    let (v, file) = (st.views.draw, st.gypsy.file);
    mem::draw_item(&mut st.video.chip, &v, 0, 0, file, 0xc7);
    passage(st, io, 0x87);
    show_passage(st);
    swap_gradient(st, io).await;
    st.video.set_palette(&st.exe, 7);
    passage(st, io, 0x88);
    until_key(st, io).await;
    gypsy_page(st, io, 0xc7, true).await;
    passage(st, io, 0x89);
    until_key(st, io).await;
    gypsy_page(st, io, 0xc7, true).await;
    until_key(st, io).await;
    gypsy_page(st, io, 0xc7, false).await;
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    swap(st, io).await;
    let s = st.views.screen;
    mem::fill_view(&mut st.video.chip, &s, 0);
    st.views.draw = st.views.screen;
    st.video.set_palette(&st.exe, 0);
}

/// 0x106f08..0x106fba: the fixed experience, level and hit points, the objlist write, the
/// "game in progress" message when the save already holds a character.
async fn finish(st: &mut IntroState, io: &Io) -> u16 {
    st.hero.exp = 0x172;
    st.hero.hp = 0x5a;
    st.hero.level = 3;
    st.views.passage = View {
        planes: Ptr::at(Region::Work),
        x1: 0x13f,
        x0: 0,
        y0: 0,
        y1: 0x37,
    };
    let mut result = 0;
    match super::handoff::write_objlist(st) {
        Ok(bytes) => st.objlist = Some(bytes),
        Err(_) => {
            passage(st, io, 0x3e6);
            show_passage(st);
            result = 1;
            swap(st, io).await;
            st.views.draw = st.views.screen;
            until_key(st, io).await;
            let d = st.views.draw;
            mem::fill_view(&mut st.video.chip, &d, 0);
            swap(st, io).await;
            st.views.draw = st.views.screen;
            let d = st.views.draw;
            mem::fill_view(&mut st.video.chip, &d, 0);
        }
    }
    super::seams::play_song(st, 0);
    result
}

//! Unit tests on synthetic data: a zeroed `u` image with the few bytes a test needs poked in,
//! a synthetic font and keyboard, and the engine as the only clock.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use u6_formats::font::WouFont;

use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::intro::exe::{A4, UExe};
use crate::intro::mem::{self, Chip, ItemHeader, Ptr, Region, View};
use crate::intro::state::IntroState;
use crate::intro::text::{self, Arg, TextCtx};
use crate::intro::{create, gypsy, handoff, input, opening, passages, screen, seams};
use crate::state::State;

/// Bytes of the synthetic image: up to the end of u's last hunk (0x11a010).
const IMAGE: usize = 0x1_a100;

fn exe() -> UExe {
    UExe::from_flat(vec![0; IMAGE])
}

/// A keyboard with the printable characters on raw keys (plain column), the named keys on
/// their hardware codes, and C type bits.
fn keyboard(st: &mut IntroState) {
    let mut table = vec![[0u16; 4]; 0x60];
    for (row, c) in (0x21..0x61u16).enumerate() {
        table[row][0] = c;
    }
    for (row, c) in (0x61..0x7fu16).enumerate() {
        table[row][1] = c;
    }
    // Lower-case letters plain on 0x20.. as well (the bridge picks the plain column first).
    for (i, c) in (b'a'..=b'z').enumerate() {
        table[0x46 + i][0] = u16::from(c);
    }
    table[0x40] = [0x20; 4];
    table[0x41] = [0x08; 4];
    table[0x44] = [0x0d; 4];
    table[0x45] = [0x1b; 4];
    st.keys.table = table;
    st.keys.ctype = (0..256u16)
        .map(|c| {
            let c = c as u8;
            if c.is_ascii_uppercase() {
                1
            } else if c.is_ascii_lowercase() {
                2
            } else if c.is_ascii_digit() {
                4
            } else if c.is_ascii_punctuation() {
                0x10
            } else {
                0x20
            }
        })
        .collect();
}

/// A font of 4×8 glyphs, column 0 ink and the rest paper; space is all paper and NUL has no
/// width (as in `u6.set`).
fn font() -> WouFont {
    let mut raw = vec![0u8; 0x304 + 64];
    raw[0] = 8;
    raw[2] = 0xf;
    raw[3] = 0;
    for c in 0..256 {
        let blank = c == 0x20 || c == 0;
        raw[4 + c] = if c == 0 { 0 } else { 4 };
        raw[0x104 + c] = if blank { 0x24 } else { 0x04 };
        raw[0x204 + c] = 0x03;
    }
    for r in 0..8 {
        raw[0x304 + r * 4] = 0xf;
    }
    WouFont::parse(&raw).unwrap()
}

fn state() -> IntroState {
    let mut st = IntroState::new(exe(), Files::default());
    keyboard(&mut st);
    st.font = Some(font());
    st
}

/// Runs `f` as the engine's game with `keys` queued at the first tick; the result once done.
fn drive<T: 'static, F: Future<Output = T> + 'static>(
    keys: &[Key],
    f: impl FnOnce(Rc<Io>) -> F,
) -> Option<T> {
    let out = Rc::new(RefCell::new(None));
    let o2 = Rc::clone(&out);
    let mut e = Engine::with_game(State::empty(), Files::default(), move |_s, io| {
        let fut = f(io);
        async move {
            *o2.borrow_mut() = Some(fut.await);
        }
    });
    let ev: Vec<InputEvent> = keys.iter().map(|&k| InputEvent::Key(k)).collect();
    e.tick(&ev, 20);
    for _ in 0..5000 {
        if e.finished() {
            break;
        }
        e.tick(&[], 20);
    }
    out.take()
}

/// A `.new` file of one item at `file`: offset table, header, the stored planes, the mask.
fn put_item(chip: &mut Chip, file: Ptr, h: ItemHeader, planes: &[&[u8]], mask: &[u8]) {
    let mut b = vec![0, 0, 0, 4];
    for w in [h.x0 as u16, h.x1, h.y0 as u16, h.y1, h.flags] {
        b.extend_from_slice(&w.to_be_bytes());
    }
    for p in planes {
        b.extend_from_slice(p);
    }
    b.extend_from_slice(mask);
    chip.write(file, &b);
}

#[test]
fn masked_blit_copies_stored_planes_clears_absent_ones_and_keeps_the_rest() {
    let mut chip = Chip::new();
    let file = Ptr::at(Region::Files);
    // 16×1 item, hot spot x0 = 2, planes 0 and 2 stored; mask: first 8 pixels.
    let h = ItemHeader {
        x0: 2,
        x1: 15,
        y0: 0,
        y1: 0,
        flags: 0b00101,
    };
    put_item(&mut chip, file, h, &[&[0xf0, 0], &[0x0f, 0]], &[0xff, 0]);
    let dst = View::screen(Ptr::at(Region::Screen0));
    mem::fill_view(&mut chip, &dst, 0b11010);
    mem::draw_item(&mut chip, &dst, 12, 5, file, 0);
    // Drawn at x = 10: pixels 10..13 plane 0, 14..17 plane 2, planes 1, 3, 4 cleared there.
    assert_eq!(chip.pixel(dst.planes, 10, 5), 0b00001);
    assert_eq!(chip.pixel(dst.planes, 14, 5), 0b00100);
    assert_eq!(chip.pixel(dst.planes, 18, 5), 0b11010, "outside the mask");
    assert_eq!(chip.pixel(dst.planes, 9, 5), 0b11010);
    // Past the table: nothing.
    mem::draw_item(&mut chip, &dst, 40, 5, file, 1);
    assert_eq!(chip.pixel(dst.planes, 38, 5), 0b11010);
    // Clipped at the bottom by the view's y1 + 1 and at the left edge.
    let low = View { y1: 4, ..dst };
    mem::draw_item(&mut chip, &low, 60, 5, file, 0);
    assert_eq!(chip.pixel(dst.planes, 58, 5), 0b11010);
    // At x = -4: pixels 0..3 of the item are off the screen, 4..7 land on x = 0..3.
    mem::draw_item(&mut chip, &dst, -2, 7, file, 0);
    assert_eq!(chip.pixel(dst.planes, 0, 7), 0b00100);
    assert_eq!(chip.pixel(dst.planes, 4, 7), 0b11010);
}

#[test]
fn copy_rect_ands_except_in_the_aligned_fast_path() {
    let mut chip = Chip::new();
    let (src, dst) = (Ptr::at(Region::Work), Ptr::at(Region::Screen0));
    mem::fill_rect(&mut chip, src, (0, 0), (32, 2), 0b01100);
    mem::fill_rect(&mut chip, dst, (0, 0), (64, 2), 0b00110);
    mem::copy_rect(&mut chip, src, (1, 0), (8, 1), dst, (3, 0), true);
    assert_eq!(chip.pixel(dst, 3, 0), 0b00100, "AND");
    assert_eq!(chip.pixel(dst, 11, 0), 0b00110, "outside the rectangle");
    mem::copy_rect(&mut chip, src, (0, 1), (16, 1), dst, (16, 1), true);
    assert_eq!(chip.pixel(dst, 16, 1), 0b01100, "fast path copies");
    mem::fill_rect(&mut chip, dst, (0, 0), (0, 5), 31);
    assert_eq!(chip.pixel(dst, 0, 0), 0b00110, "zero width fills nothing");
}

#[test]
fn glyph_values_draw_with_bit_1_doubled_into_plane_4() {
    assert_eq!(text::glyph_colour(2), 18);
    assert_eq!(text::glyph_colour(7), 23);
    assert_eq!(text::glyph_colour(0x1e), 30);
    assert_eq!(text::glyph_colour(0x15), 5);
}

fn ctx(view: View) -> TextCtx {
    TextCtx {
        view,
        x: view.x0,
        y: view.y0,
        ..TextCtx::default()
    }
}

#[test]
fn wrap_centres_and_justifies_lines() {
    let f = font();
    let mut chip = Chip::new();
    let v = View {
        planes: Ptr::at(Region::Screen0),
        x1: 40,
        x0: 0,
        y0: 0,
        y1: 100,
    };
    // "ab cd" = 20 px in a 41 px line, centred: start at (41 - 20 + 1) / 2 = 11.
    let mut c = ctx(v);
    c.align = 2;
    text::wrap(&mut chip, &mut c, &f, b"ab cd");
    let ink = text::glyph_colour(15);
    assert_eq!(chip.pixel(v.planes, 11, 0), ink);
    assert_eq!(chip.pixel(v.planes, 10, 0), 0);
    // Justified: "aa bb cc dd" breaks after "aa bb cc" (32 px of 41: slack 9 over the two
    // spaces after the first word, 5 + 4). The last line is drawn as it is.
    let mut chip = Chip::new();
    let mut c = ctx(v);
    c.align = 3;
    text::wrap(&mut chip, &mut c, &f, b"aa bb cc dd");
    let line: Vec<i32> = (0..41)
        .filter(|&x| chip.pixel(v.planes, x, 0) == ink)
        .collect();
    assert_eq!(line, [0, 4, 17, 21, 33, 37]);
    let ink2: Vec<i32> = (0..41)
        .filter(|&x| chip.pixel(v.planes, x, 8) == ink)
        .collect();
    assert_eq!(ink2, [0, 4]);
}

#[test]
fn printf_codes_colour_cursor_and_arguments() {
    let f = font();
    let mut chip = Chip::new();
    let v = View::screen(Ptr::at(Region::Screen0));
    let mut c = ctx(v);
    text::print_new(
        &mut chip,
        &mut c,
        &f,
        b"%F%s %d%c",
        &[
            Arg::Word(3),
            Arg::Str(b"hi".to_vec()),
            Arg::Word(0xfffe),
            Arg::Word(0x41),
        ],
    );
    assert_eq!(c.buf, b"hi -2A");
    assert_eq!(c.fg, 3);
    text::print(
        &mut chip,
        &mut c,
        &f,
        b"%X%Y%B",
        &[Arg::Word(7), Arg::Word(9), Arg::Word(1)],
    );
    assert_eq!((c.x, c.y, c.bg), (7, 9, 1));
    c.align = 0;
    text::print(&mut chip, &mut c, &f, b"%P", &[]);
    assert_eq!(
        chip.pixel(v.planes, 7, 9),
        text::glyph_colour(3),
        "flushed at the cursor in ink 3"
    );
}

#[test]
fn raw_keys_map_with_qualifiers_and_the_ring_drops_when_full() {
    let mut st = state();
    st.keys.table[0x10] = [u16::from(b'q'), u16::from(b'Q'), 0x11, 0];
    let k = &mut st.keys;
    assert_eq!(k.map_raw(0x10), u16::from(b'q'));
    assert_eq!(k.map_raw(0x90), 0, "release");
    k.map_raw(0x60);
    assert_eq!(k.map_raw(0x10), u16::from(b'Q'));
    k.map_raw(0xe0);
    k.map_raw(0x63);
    assert_eq!(k.map_raw(0x10), 0x11, "control column");
    k.map_raw(0xe3);
    k.map_raw(0x62);
    assert_eq!(k.map_raw(0x10), u16::from(b'Q'), "caps lock upper-cases");
    for _ in 0..120 {
        k.queue(7);
    }
    k.queue(0);
    let taken = drive(&[], move |io| async move {
        let mut n = 0;
        while st.keys.ready(&io) {
            st.keys.take(&io);
            n += 1;
        }
        n
    });
    assert_eq!(taken, Some(99), "one slot stays empty");
}

#[test]
fn fades_step_once_per_frame() {
    let mut st = state();
    st.video.start_fade_in([0x0abc_u16; 32]);
    st.video.fade_in_step();
    assert_eq!(st.video.colours[0], 0, "first step: level 0xfff");
    for _ in 0..14 {
        st.video.fade_in_step();
    }
    assert_eq!(st.video.colours[0], 0x0abc - 0x0111);
    st.video.fade_in_step();
    assert_eq!(st.video.colours[0], 0x0abc);
    assert!(st.video.fade_in_count < 0);
    st.video.start_fade_out([0x0302; 32]);
    st.video.fade_out_step();
    assert_eq!(st.video.colours[0], 0x0302);
    st.video.fade_out_step();
    assert_eq!(st.video.colours[0], 0x0201);
    // 0x11 counts down to −1: 18 steps in all.
    for _ in 0..15 {
        st.video.fade_out_step();
    }
    assert_eq!(st.video.fade_out_count, 0);
    st.video.fade_out_step();
    assert!(st.video.fade_out_count < 0);
    assert_eq!(st.video.colours[0], 0);
    st.video.cycle_palette[22..26].copy_from_slice(&[1, 2, 3, 4]);
    st.video.cycling = 1;
    st.video.cycle_wait = 1;
    st.video.server();
    assert_eq!(&st.video.colours[22..26], &[2, 3, 4, 1]);
    assert_eq!(st.video.cycle_wait, 3);
}

#[test]
fn swaps_alternate_the_two_buffers() {
    let mut st = state();
    assert_eq!(
        (st.video.shown, st.video.draw),
        (Region::Screen0, Region::Screen1)
    );
    let p = st.video.swap(&exe());
    assert_eq!(
        (st.video.shown, p.region),
        (Region::Screen1, Region::Screen0)
    );
    assert_eq!(st.video.other().region, Region::Screen1);
    assert_eq!(st.video.flip().region, Region::Screen1);
    assert_eq!(
        st.video.draw,
        Region::Screen0,
        "a flip leaves the drawing pointer"
    );
}

#[test]
fn c_rand_and_the_shimmer_generator() {
    let mut st = state();
    st.srand(1);
    // 1 × 0x41c64e6d + 0x3039 = 0x41c67ea6 → bits 16..30.
    assert_eq!(st.rand(), 0x41c6);
    assert_eq!(st.random(0), 0);
    st.noise = 0x8000_0000;
    assert_eq!(st.noise_word(), 0x1d87, "carry: xor");
    st.noise = 1;
    assert_eq!(st.noise_word(), 0, "no carry, nonzero: shift only");
    assert_eq!(st.noise, 2);
}

/// Pokes big-endian words at `addr`.
fn poke_words(e: &mut UExe, addr: u32, words: &[u16]) {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
    e.poke(addr, &bytes);
}

/// The displacement word of a PC-relative operand whose extension word is at `ext`.
fn rel(ext: u32, to: u32) -> u16 {
    to.wrapping_sub(ext) as u16
}

/// A passage switch with case 0 and a default, the two thunks, and a format string.
fn passage_exe() -> UExe {
    let mut e = exe();
    let chain = 0x0010_ac7a;
    let case0 = 0x0010_ae00;
    let tail = 0x0010_c7f4;
    let fill_thunk = 0x0010_f51a;
    let print_thunk = 0x0010_f532;
    let fmt = 0x0010_c80c;
    poke_words(
        &mut e,
        chain,
        &[
            0x2007,
            0x4a40,
            0x6700,
            rel(chain + 6, case0),
            0x6000,
            rel(chain + 10, tail),
        ],
    );
    let mut code: Vec<u16> = vec![
        0x7005, 0x3940, 0x2cfc, 0x397c, 0x012f, 0x2d0a, 0x2006, 0x41ec, 0x2d02,
    ];
    let at = |code: &Vec<u16>| case0 + 2 * code.len() as u32;
    let ext = at(&code) + 2;
    code.extend([0x4eba, rel(ext, fill_thunk)]);
    code.extend([0x554f, 0x707f, 0x3f00]);
    let ext = at(&code) + 2;
    code.extend([0x487a, rel(ext, fmt)]);
    let ext = at(&code) + 2;
    code.extend([0x4eba, rel(ext, print_thunk)]);
    code.extend([0x504f, 0x1b7c, 0x0002, 0xfffe]);
    let ext = at(&code) + 2;
    code.extend([0x6000, rel(ext, tail)]);
    poke_words(&mut e, case0, &code);
    poke_words(&mut e, tail, &[0x206d, 0xfff6]);
    poke_words(&mut e, fill_thunk, &[0x4ef9, 0x0010, 0xf898]);
    poke_words(&mut e, print_thunk, &[0x4ef9, 0x0011, 0x42f0]);
    e.poke(fmt, b"Hi%c\0");
    e
}

#[test]
fn passages_are_read_out_of_the_case_code() {
    let e = passage_exe();
    assert_eq!(passages::case_address(&e, 0), Some(0x0010_ae00));
    assert_eq!(passages::case_address(&e, 7), Some(0x0010_c7f4), "default");
    assert_eq!(
        passages::case_address(&exe(), 0),
        None,
        "not the expected chain"
    );
    let mut st = IntroState::new(e, Files::default());
    keyboard(&mut st);
    st.font = Some(font());
    st.views.passage = View::screen(Ptr::at(Region::Work));
    let st = drive(&[], move |io| async move {
        passages::passage(&mut st, &io, 0);
        st
    })
    .unwrap();
    assert_eq!(st.frame.x, 5);
    assert_eq!(st.views.passage.x1, 0x12f);
    // Filled with D6 = 0x1e, then "Hi" + glyph 0x7f centred (align 2) in ink 0 on 0x1e.
    let p = st.views.passage.planes;
    assert_eq!(st.video.chip.pixel(p, 300, 4), 0x1e);
    let ink: Vec<i32> = (0..0x130)
        .filter(|&x| st.video.chip.pixel(p, x, 0) == 0)
        .collect();
    assert_eq!(ink, [146, 150, 154], "(304 - 12 + 1) / 2 = 146");
}

#[test]
fn read_line_takes_accepted_keys_backspaces_and_cancels() {
    let mut st = state();
    st.text = ctx(View::screen(Ptr::at(Region::Screen0)));
    let keys = [
        Key::Char('a'),
        Key::Char('!'),
        Key::Char('b'),
        Key::Backspace,
        Key::Char('c'),
        Key::Enter,
    ];
    let got = drive(&keys, move |io| async move {
        create::read_line(&mut st, &io, 13).await
    });
    // '!' is punctuation the line does not take (only - ' . , besides letters and digits).
    assert_eq!(got, Some(Some(b"ac".to_vec())));
    let mut st = state();
    let got = drive(&[Key::Enter], move |io| async move {
        create::read_line(&mut st, &io, 13).await
    });
    assert_eq!(got, Some(None), "Return on an empty line cancels");
    let mut st = state();
    let got = drive(&[Key::Char('m')], move |io| async move {
        create::read_line(&mut st, &io, 1).await
    });
    assert_eq!(
        got,
        Some(Some(b"m".to_vec())),
        "one character returns at once"
    );
}

#[test]
fn the_sex_answer_is_spelled_out_by_two_toggles() {
    let mut e = exe();
    e.poke(0x0010_99a2, b"Male\0\0Female\0");
    let mut st = IntroState::new(e, Files::default());
    st.hero.sex = b"M".to_vec();
    create::toggle_sex(&mut st);
    assert_eq!(st.hero.sex, b"Female");
    create::toggle_sex(&mut st);
    assert_eq!(st.hero.sex, b"Male");
}

#[test]
fn the_tv_script_draws_frames_until_a_step_ends() {
    let mut e = exe();
    // Script 0: frame 5, end of step; a loop of 2 over (frame 6, repeat); next script.
    e.poke(A4 + 0x1c6, &[0x05, 0x8a, 0x80, 0x02, 0x06, 0x81, 0x85]);
    e.poke(A4 + 0x1c6 + 0x50, &[0x82]);
    let mut st = IntroState::new(e, Files::default());
    st.tv = opening::Tv::new(&st);
    opening::set_script(&mut st, 0);
    opening::tv_step(&mut st);
    assert_eq!((st.tv.frame, st.tv.pos), (5, 2));
    opening::tv_step(&mut st);
    assert_eq!((st.tv.frame, st.tv.pos, st.tv.repeat), (6, 4, 1));
    opening::tv_step(&mut st);
    assert_eq!((st.tv.pos, st.tv.repeat), (4, 0));
    opening::tv_step(&mut st);
    assert_eq!(st.tv.pos, 6, "repeat count spent: falls through");
    opening::tv_step(&mut st);
    assert_eq!(st.tv.script, 1, "0x85 goes to the next script");
    assert_eq!(st.tv.static_drawn, 1, "script 1 starts with static");
    assert_eq!(st.tv.frame, -1);
}

#[test]
fn a_digit_key_switches_the_tv_script() {
    let mut st = state();
    st.tv = opening::Tv::new(&st);
    let got = drive(&[Key::Char('9'), Key::Escape], move |io| async move {
        let a = screen::check_key(&mut st, &io);
        let script = st.tv.script;
        let b = screen::check_key(&mut st, &io);
        (a, script, b, st.skip)
    });
    assert_eq!(got, Some((0, 2, 0x1b, 1)));
}

#[test]
fn songs_switch_once() {
    let mut st = state();
    seams::play_song(&mut st, 2);
    assert_eq!(st.song, 2);
    seams::play_song(&mut st, 2);
    assert_eq!(st.song, 2);
    seams::play_song(&mut st, 0);
    assert_eq!(st.song, 0);
}

#[test]
fn pours_share_the_shimmer_colours_and_lift_the_shimmer() {
    let mut st = state();
    st.views.shimmer.y0 = 100;
    gypsy::add_pour(&mut st, 10);
    assert_eq!(st.gypsy.shimmer_colours, [10; 16]);
    assert_eq!(st.views.shimmer.y0, 96);
    gypsy::add_pour(&mut st, 4);
    // 16 / 2 = 8 entries each: pour 1 at 0..7, pour 2 at 0, 2, .., 14.
    assert_eq!(st.gypsy.shimmer_colours[..4], [4, 10, 4, 10]);
    assert_eq!(st.gypsy.shimmer_colours[8..12], [4, 10, 4, 10]);
}

#[test]
fn shuffle_swaps_n_plus_a_random_number_of_times() {
    let mut a = state();
    a.srand(77);
    a.gypsy.order = [0, 1, 2, 3, 4, 5, 6, 7];
    gypsy::shuffle(&mut a, 8);
    let mut sorted = a.gypsy.order;
    sorted.sort_unstable();
    assert_eq!(sorted, [0, 1, 2, 3, 4, 5, 6, 7]);
    let mut b = state();
    b.srand(77);
    // The same draws by hand: 8 + rand % 64 swaps of (rand % 8, rand % 8).
    let swaps = 8 + b.random(64);
    let mut order = [0i16, 1, 2, 3, 4, 5, 6, 7];
    for _ in 0..swaps {
        let x = b.random(8) as usize;
        let y = b.random(8) as usize;
        order.swap(x, y);
    }
    assert_eq!(a.gypsy.order, order);
}

#[test]
fn questions_come_from_the_pair_table() {
    let mut e = exe();
    e.poke(A4 + 0x426 + 16 * 2 + 2 * 5, &0x78u16.to_be_bytes());
    let st = IntroState::new(e, Files::default());
    assert_eq!(gypsy::question(&st, 2, 5), 0x78);
}

#[test]
fn the_objlist_gets_the_character_and_refuses_a_game_in_progress() {
    let mut list = vec![0u8; u6_formats::objlist::OBJLIST_LEN];
    list[0x1d70] = 0xaa;
    let files = Files::new([("PLAYER/objlist".to_string(), list.clone())].into());
    let mut st = IntroState::new(exe(), files);
    st.hero.name[..5].copy_from_slice(b"Zhuge");
    st.hero.sex = b"Female".to_vec();
    (st.hero.str, st.hero.dex, st.hero.int) = (17, 19, 21);
    (st.hero.exp, st.hero.hp, st.hero.level) = (0x172, 300, 3);
    st.hero.portrait_item = 4;
    st.hero.karma = 78;
    let b = handoff::write_objlist(&st).unwrap();
    assert_eq!(&b[0xf00..0xf06], b"Zhuge\0");
    assert_eq!((b[0x901], b[0xa01], b[0xb01], b[0x13f2]), (17, 19, 21, 42));
    assert_eq!((b[0xc02], b[0xc03], b[0xe01], b[0xff2]), (1, 0x72, 0xff, 3));
    assert_eq!((b[0x1c71], b[0x1c72], b[0x1bf9]), (1, 5, 78));
    assert_eq!(b[0x1d70], 0xaa, "past 0x1c73 the file is kept");
    let mut s = State::empty();
    s.actors.party.names = vec![String::new(); 16];
    handoff::apply(&mut s, &b).unwrap();
    assert_eq!(s.actors.party.names[0], "Zhuge");
    assert_eq!(
        (s.actors.attrs.strength[1], s.actors.attrs.exp[1]),
        (17, 0x172)
    );
    assert_eq!((s.actors.attrs.hp[1], s.actors.attrs.magic[1]), (0xff, 42));
    assert_eq!((s.actors.karma, s.actors.objlist_tail[0x81]), (78, 5));
    assert!(handoff::summary(&b).contains("\"Zhuge\""));
    list[0x1c72] = 1;
    let files = Files::new([("PLAYER/objlist".to_string(), list)].into());
    let st = IntroState::new(exe(), files);
    assert_eq!(
        handoff::write_objlist(&st),
        Err(handoff::WriteError::InProgress)
    );
}

#[test]
fn the_menu_takes_letters_and_return() {
    for (key, want) in [('c', 1), ('i', 0), ('t', 2), ('a', 3), ('j', 4)] {
        let mut st = state();
        st.menu_drawn = 1;
        let got = drive(&[Key::Char(key)], move |io| async move {
            crate::intro::menu(&mut st, &io).await
        });
        assert_eq!(got, Some(want), "{key}");
    }
    let mut st = state();
    st.menu_drawn = 1;
    let got = drive(&[Key::Char('x'), Key::Enter], move |io| async move {
        crate::intro::menu(&mut st, &io).await
    });
    assert_eq!(got, Some(0), "Return takes the first entry");
}

#[test]
fn input_upper_cases_like_the_c_runtime() {
    assert_eq!(input::to_upper(u16::from(b'a')), u16::from(b'A'));
    assert_eq!(input::to_upper(u16::from(b'{')), u16::from(b'{'));
    assert_eq!(input::to_upper(0xe9), 0xe9);
}

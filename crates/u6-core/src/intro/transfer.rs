//! Transfer a character from an Ultima IV or Ultima V save (0x10a81e and its helpers in hunk 4).
//!
//! The dialog 0x10a450 asks for the save disk in df0: and A (Ultima IV) or B (Ultima V). The
//! original reads, into the scene buffer + 100000:
//!
//! - Ultima IV: the whole file `PLAY:ultmapp/rost.bin` through dos.library (Open, Seek to the
//!   end for the size, Seek back, Read; 0x10f6ae, 0x10f716), the roster of the Ultima IV
//!   player disk.
//! - Ultima V: `saved.gam` from the Ultima V disk, which is not an AmigaDOS disk: 0x1148b0
//!   takes the machine over (interrupts and DMA off), steps the df0: head itself, reads raw MFM
//!   tracks (sync word 0x8915, 11 blocks of 512 bytes a track, 0x1622 the decoded track), looks
//!   the name up in the 160 32-byte directory entries of track 0 (16-byte name, size, start
//!   track and block) and follows the files' 508-byte chained blocks; any read error gives no
//!   file.
//!
//! The port reads both from the user's files by those names (the bare name decides, so
//! `rost.bin` and `saved.gam` added to the files are found); without
//! them the original's path for no character found runs. The record found (0x109870 /
//! 0x109ad6) is shown next to what it becomes in Ultima VI (0x109bea / 0x109c80) on a sheet
//! where the name and sex can be changed (0x109e72); then the portrait is chosen (0x10a15e)
//! and the character is written into the objlist (0x105700) like a created one.

use crate::engine::Io;

use super::create::{FEMALE, MALE, choose, choose_portrait, read_line, toggle_sex};
use super::exe::A4;
use super::mem::{self, Ptr, Region, View};
use super::music;
use super::passages::{passage, show_passage};
use super::screen::{poll_key, swap};
use super::state::IntroState;
use super::text::{self, Arg};

/// Strings of `u` the transfer prints or opens.
const TITLE: u32 = 0x0010_9a1a;
const PUT_DISK: u32 = 0x0010_9a32;
const CLICK_A: u32 = 0x0010_9a5a;
const OR_B: u32 = 0x0010_9a7a;
const U4_ROSTER: u32 = 0x0010_9a92;
const NOT_FOUND: u32 = 0x0010_9aa8;
const U5_SAVE: u32 = 0x0011_4932;
const ULTIMA: u32 = 0x0010_99ce;
const IV: u32 = 0x0010_99d6;
const V: u32 = 0x0010_99da;
const ULTIMA_VI: u32 = 0x0010_99dc;
const LABELS: u32 = 0x0010_99e6;
const LABELS_2: u32 = 0x0010_99f6;
const FMT_NAMES: u32 = 0x0010_99b0;
const FMT_NUMBERS: u32 = 0x0010_99ba;
/// Original: A4+0x94b (the Ultima IV class names, 9 bytes each: the eight classes, then the
/// Avatar's)
const CLASSES: u32 = A4 + 0x94b;
/// Original: A4+0x993 (the Avatar's class name)
const AVATAR: u32 = A4 + 0x993;
/// Original: A4+0x99c (the sheet's keys: N, S, C, A, Escape)
const SHEET_KEYS: u32 = A4 + 0x99c;

/// Where the save came from (A4+0x2c50).
pub mod source {
    pub const NONE: u16 = 0;
    pub const ULTIMA_IV: u16 = 4;
    pub const ULTIMA_V: u16 = 5;
}

/// Original: A4+0x2c14 .. A4+0x2c4c (the character as the save has it)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// Original: A4+0x2c14 (16 bytes: the name; 8 copied)
    pub name: [u8; 16],
    /// Original: A4+0x2c24 / A4+0x2c26 / A4+0x2c28
    pub str: i16,
    pub dex: i16,
    pub int: i16,
    /// Original: A4+0x2c2a (the male or female word, 0x1099a2 / 0x1099a8)
    pub sex: Vec<u8>,
    /// Original: A4+0x2c32 (the class name)
    pub class: Vec<u8>,
    /// Original: A4+0x2c42 (byte: status, 'G')
    pub status: u8,
    /// Original: A4+0x2c43 (byte: the save's player is the Avatar)
    pub avatar: u8,
    /// Original: A4+0x2c44 / A4+0x2c46 / A4+0x2c48 / A4+0x2c4a / A4+0x2c4c
    pub exp: i16,
    pub level: i16,
    pub hp: i16,
    pub max_hp: i16,
    pub magic: i16,
}

impl Found {
    /// The name as a C string (up to the NUL).
    pub fn name_str(&self) -> Vec<u8> {
        self.name.iter().copied().take_while(|&b| b != 0).collect()
    }
}

/// The transfer's globals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transfer {
    /// Original: A4+0x2c50 (see [`source`])
    pub source: u16,
    /// Original: A4+0x2bd4 (the save's bytes; the original's pointer into the scene buffer)
    pub save: Vec<u8>,
    pub found: Found,
}

fn byte(b: &[u8], i: usize) -> u8 {
    b.get(i).copied().unwrap_or(0)
}

fn word(b: &[u8], i: usize) -> i16 {
    i16::from_be_bytes([byte(b, i), byte(b, i + 1)])
}

/// Original: u 0x109870 (transfer: the Ultima IV roster's first player into A4+0x2c14) — true
/// when a value is out of range (the caller ignores it)
///
/// Name bytes 0..8, sex byte 0x10 ('\\' = male), strength / dexterity / intelligence 0x13..0x15,
/// magic 0x16, hit points 0x18, maximum 0x1a (level = maximum / 100), experience 0x1c, class 0x11
/// (8 = Avatar when the second record's name is empty, written back into the roster).
pub fn read_ultima_iv(st: &mut IntroState) -> bool {
    let save = &mut st.transfer.save;
    if save.len() < 0x108 {
        save.resize(0x108, 0);
    }
    let f = &mut st.transfer.found;
    // Each byte, then a NUL after it (so the ninth byte ends up 0).
    for (i, &b) in save[..8].iter().enumerate() {
        f.name[i] = b;
        f.name[i + 1] = 0;
    }
    f.str = i16::from(save[0x13]);
    f.dex = i16::from(save[0x14]);
    f.int = i16::from(save[0x15]);
    let sex = if save[0x10] == 0x5c { MALE } else { FEMALE };
    f.sex = st.exe.cstr(sex);
    f.status = b'G';
    f.avatar = u8::from(save[0x100..0x108].iter().all(|&b| b == 0));
    if f.avatar != 0 {
        save[0x11] = 8;
    }
    let class = save[0x11];
    f.class = st.exe.cstr(CLASSES + 9 * u32::from(class));
    f.exp = word(save, 0x1c);
    f.level = (word(save, 0x1a) as u16 / 100) as i16;
    f.hp = word(save, 0x18);
    f.max_hp = word(save, 0x1a);
    f.magic = i16::from(save[0x16]);
    f.str > 99
        || f.dex > 99
        || f.int > 99
        || f.hp > 9999
        || f.max_hp > 9999
        || f.exp > 9999
        || class > 7
}

/// Original: u 0x109ad6 (transfer: the Ultima V save's first character (record at byte 2) into
/// A4+0x2c14) — true when a value is out of range (the caller ignores it)
///
/// Name bytes 0..8, sex byte 9 (0xb = male), strength / dexterity / intelligence 0xc..0xe,
/// magic 0xf, hit points 0x10, maximum 0x12, experience 0x14, level 0x16; the class is Avatar.
pub fn read_ultima_v(st: &mut IntroState) -> bool {
    let r: Vec<u8> = (0..0x17).map(|i| byte(&st.transfer.save, 2 + i)).collect();
    let f = &mut st.transfer.found;
    f.name[..8].copy_from_slice(&r[..8]);
    f.str = i16::from(r[0xc]);
    f.dex = i16::from(r[0xd]);
    f.int = i16::from(r[0xe]);
    let sex = if r[9] == 0xb { MALE } else { FEMALE };
    f.sex = st.exe.cstr(sex);
    f.status = b'G';
    f.avatar = 1;
    f.class = st.exe.cstr(AVATAR);
    f.exp = word(&r, 0x14);
    f.level = i16::from(r[0x16]);
    f.hp = word(&r, 0x10);
    f.max_hp = word(&r, 0x12);
    f.magic = i16::from(r[0xf]);
    f.str > 30 || f.dex > 30 || f.int > 30 || f.hp > 9999 || f.max_hp > 9999 || f.exp > 9999
}

/// Original: u 0x109ba8 (transfer: an Ultima IV attribute in Ultima VI's range: below 10 as it
/// is, 10..29 → 10 + (v − 9) / 2, from 30 on → 20 + (v − 30) / 4)
pub const fn ultima_iv_attribute(v: i16) -> i16 {
    if v < 10 {
        v
    } else if v >= 30 {
        (v - 30) / 4 + 20
    } else {
        (v - 9) / 2 + 10
    }
}

/// The part of 0x109bea / 0x109c80 both share: name, sex, class, status, then experience / 10,
/// level 1 + one per halving of experience / 1000, hit points 30 per level, magic = intelligence.
fn convert(st: &mut IntroState, attrs: [i16; 3]) {
    let f = st.transfer.found.clone();
    let name = f.name_str();
    let n = name.len().min(st.hero.name.len() - 1);
    st.hero.name[..n].copy_from_slice(&name[..n]);
    st.hero.name[n] = 0;
    let h = &mut st.hero;
    [h.str, h.dex, h.int] = attrs;
    h.sex = f.sex.clone();
    h.class = st.exe.cstr(AVATAR);
    h.status = b'G';
    h.exp = f.exp / 10;
    h.level = 1;
    let mut d = f.exp / 1000;
    while d > 0 {
        h.level += 1;
        d >>= 1;
    }
    h.hp = h.level.wrapping_mul(30);
    h.max_hp = h.hp;
    h.magic = h.int;
    h.transferred = 1;
}

/// Original: u 0x109bea (transfer: an Ultima IV character into the creation's globals; the
/// attributes through [`ultima_iv_attribute`])
pub fn convert_ultima_iv(st: &mut IntroState) {
    let f = &st.transfer.found;
    let attrs = [f.str, f.dex, f.int].map(ultima_iv_attribute);
    convert(st, attrs);
}

/// Original: u 0x109c80 (transfer: an Ultima V character into the creation's globals; the
/// attributes as they are)
pub fn convert_ultima_v(st: &mut IntroState) {
    let f = &st.transfer.found;
    let attrs = [f.str, f.dex, f.int];
    convert(st, attrs);
}

/// `printf` through the main text context, restarting the buffer (0x1142c6), appending
/// (0x1142f0) or straight to the screen (0x1142a6).
#[derive(Clone, Copy)]
enum Print {
    New,
    More,
    Screen,
}

fn print(st: &mut IntroState, how: Print, fmt: u32, args: &[Arg]) {
    let fmt = st.exe.cstr(fmt);
    if let Some(font) = st.font.as_ref() {
        let (chip, ctx) = (&mut st.video.chip, &mut st.text);
        match how {
            Print::New => text::print_new(chip, ctx, font, &fmt, args),
            Print::More => text::print_more(chip, ctx, font, &fmt, args),
            Print::Screen => text::print(chip, ctx, font, &fmt, args),
        }
    }
}

/// The dialog's three boxes (the locals −0x1a, −0x2a, −0x3a of 0x10a450): title, text,
/// buttons.
fn dialog_boxes(planes: Ptr) -> [View; 3] {
    let at = |y0, y1| View {
        planes,
        x1: 0x124,
        x0: 0x1e,
        y0,
        y1,
    };
    [at(0x59, 0x64), at(0x64, 0xae), at(0xae, 0xbd)]
}

/// Original: u 0x10a450 (transfer: ask for the save disk and A / B; read the save) — the
/// source (4 Ultima IV, 5 Ultima V), or 0 for Escape and when no character was found
///
/// Without a save the bell rings (a BEL to the console, which the port has not), the not-found
/// line (the string at 0x109aa8) replaces the text and a key ends the dialog.
///
/// Replaces: u 0x10f6ae (seek and read), u 0x10f716 (close), u 0x1148b0 (the raw Ultima V disk
/// reader) with u 0x11493c, u 0x114982, u 0x1149d0, u 0x114a40, u 0x114a5a, u 0x114a66,
/// u 0x114a84, u 0x114ae4, u 0x114b58, u 0x114c88, u 0x114cc6, u 0x114cf4 (by the files)
pub async fn ask_source(st: &mut IntroState, io: &Io) -> u16 {
    st.views.draw.planes = st.views.screen.planes;
    let [mut title, mut body, mut row] = dialog_boxes(st.views.draw.planes);
    let p = st.views.portrait;
    mem::fill_view(&mut st.video.chip, &p, 0x1e);
    st.text.fg = 0;
    st.text.bg = 0x1e;
    st.text.view = title;
    st.text.x = title.x0;
    st.text.y = title.y0;
    st.text.align = 2;
    print(st, Print::New, TITLE, &[]);
    body.planes = st.views.screen.planes;
    st.text.view = body;
    st.text.x = body.x0;
    st.text.y = title.y0 + 0x20;
    mem::fill_view(&mut st.video.chip, &body, 0x1e);
    print(st, Print::New, PUT_DISK, &[]);
    print(st, Print::More, CLICK_A, &[]);
    print(st, Print::More, OR_B, &[]);
    // The rule under the title: the text box's top less 2, one row.
    let line = View {
        x0: body.x0,
        x1: body.x1 - 1,
        y0: body.y0 - 2,
        y1: body.y0 - 2,
        ..st.views.screen
    };
    mem::fill_view(&mut st.video.chip, &line, 0);
    let vellum = st.files_ptr;
    let mut keys = Vec::new();
    for i in 0..2i16 {
        let x0 = i * 0x11 + 0x1f;
        mem::draw_item(&mut st.video.chip, &row, x0, row.y0, vellum, (i + 5) as u16);
        keys.push(b'A' + i as u8);
    }
    keys.push(0x1b);
    mem::draw_item(&mut st.video.chip, &row, 0xdd, row.y0, vellum, 4);
    let planes = st.views.screen.planes;
    for v in [&mut title, &mut body, &mut row] {
        v.planes = planes;
    }
    swap(st, io).await;
    st.video.set_palette(&st.exe, 6);
    st.video.pointer = 1;
    st.transfer.source = source::NONE;
    let key = keys[choose(st, io, &keys).await];
    if key == 0x1b {
        return 0;
    }
    if key == b'A' {
        let name = st.exe.cstr(U4_ROSTER);
        if let Some(save) = super::load::load_raw(&st.files, &name) {
            st.transfer.save = save;
            st.transfer.source = source::ULTIMA_IV;
        }
    }
    if key == b'B' {
        music::play_song(st, 0);
        let name = st.exe.cstr(U5_SAVE);
        if let Some(save) = super::load::load_raw(&st.files, &name) {
            st.transfer.save = save;
            st.transfer.source = source::ULTIMA_V;
        }
        music::play_song(st, 4);
    }
    if st.transfer.source == source::NONE {
        st.video.pointer = 0;
        st.text.x = body.x0;
        st.text.y = title.y0 + 0x20;
        body.y1 = 0xbd;
        st.text.view = body;
        mem::fill_view(&mut st.video.chip, &body, 0x1e);
        print(st, Print::New, NOT_FOUND, &[]);
        while poll_key(st, io) == 0 {
            st.video.wait_frame(io).await;
        }
    }
    st.video.pointer = 0;
    st.transfer.source
}

/// Original: u 0x109d0a (transfer sheet: the values, the save's in ink 2 and Ultima VI's in ink
/// 9, then the four buttons)
///
/// The value box A4+0x2c9c moves right for each column (the text context points at it) and
/// back at the end.
pub fn values(st: &mut IntroState) {
    let b = &mut st.views.buttons;
    b[1].x0 += 0x35;
    let v = b[1];
    st.text.view = v;
    mem::fill_view(&mut st.video.chip, &v, 0x1e);
    st.text.fg = 2;
    st.text.bg = 0x1e;
    st.text.x = v.x0;
    st.text.y = v.y0 + 2;
    st.text.align = 0;
    let f = st.transfer.found.clone();
    let strs = |a: &[u8], b: &[u8], c: &[u8]| {
        vec![
            Arg::Str(a.to_vec()),
            Arg::Str(b.to_vec()),
            Arg::Str(c.to_vec()),
        ]
    };
    let nums = |n: [i16; 6]| n.map(|v| Arg::Word(v as u16)).to_vec();
    print(
        st,
        Print::New,
        FMT_NAMES,
        &strs(&f.name_str(), &f.sex, &f.class),
    );
    let row = [f.str, f.dex, f.int, f.magic, f.exp, f.level];
    print(st, Print::More, FMT_NUMBERS, &nums(row));
    st.views.buttons[1].x0 += 0x20;
    st.views.buttons[1].x0 += 0x31;
    let v = st.views.buttons[1];
    st.text.view = v;
    st.text.fg = 9;
    st.text.x = v.x0;
    st.text.y = v.y0 + 2;
    let h = st.hero.clone();
    print(
        st,
        Print::New,
        FMT_NAMES,
        &strs(&h.name_str(), &h.sex, &h.class),
    );
    let row = [h.str, h.dex, h.int, h.magic, h.exp, h.level];
    print(st, Print::More, FMT_NUMBERS, &nums(row));
    st.views.buttons[1].x0 -= 0x86;
    st.text.view = st.views.buttons[1];
    st.views.portrait.planes = st.views.screen.planes;
    let (p, vellum, b) = (st.views.portrait, st.files_ptr, st.views.buttons);
    for (k, item) in [(2usize, 2u16), (3, 3), (4, 0x10), (5, 4)] {
        mem::draw_item(&mut st.video.chip, &p, b[k].x0, b[k].y0, vellum, item);
    }
}

/// The sheet's boxes (A4+0x2c8c .. A4+0x2cec): the heading line, the value box, the four
/// buttons N, S, C, A and a copy of the last.
fn sheet_boxes(planes: Ptr) -> [View; 7] {
    let at = |x0, x1, y0, y1| View {
        planes,
        x1,
        x0,
        y0,
        y1,
    };
    let last = at(0xdd, 0x124, 0xae, 0xbd);
    [
        at(0x1e, 0x123, 0x57, 0x63),
        at(0x1e, 0x123, 0x63, 0xae),
        at(0x1c, 0x5d, 0xae, 0xbd),
        at(0x5f, 0x98, 0xae, 0xbd),
        at(0x9a, 0xdb, 0xae, 0xbd),
        last,
        last,
    ]
}

/// The value box redrawn after an edit (0x10a0e2 .. 0x10a108 and 0x10a11c .. 0x10a142).
fn redraw_values(st: &mut IntroState) {
    st.views.buttons[1].planes = st.views.draw.planes;
    st.views.buttons[1].planes = st.views.screen.planes;
    values(st);
    st.views.buttons[1].planes = st.views.screen.planes;
    st.video.pointer = 1;
}

/// Original: u 0x109e72 (transfer: the sheet, the save's character next to the Ultima VI one;
/// N renames, S switches the sex, C accepts, A or Escape aborts) — true on accept
pub async fn sheet(st: &mut IntroState, io: &Io) -> bool {
    st.views.portrait.planes = st.views.screen.planes;
    st.video.pointer = 0;
    st.views.buttons = sheet_boxes(st.views.draw.planes);
    let keep_y1 = st.views.buttons[1].y1;
    let planes = st.views.screen.planes;
    let b = &mut st.views.buttons;
    b[1].planes = planes;
    b[0].planes = planes;
    b[1].y0 -= 1;
    b[1].y1 = b[1].y0;
    let (p, line) = (st.views.portrait, st.views.buttons[1]);
    mem::fill_view(&mut st.video.chip, &p, 0x1e);
    mem::fill_view(&mut st.video.chip, &line, 0x1e);
    st.text.fg = 0;
    st.text.bg = 0x1e;
    let b = &mut st.views.buttons;
    b[1].y1 = keep_y1;
    b[1].y0 += 1;
    st.text.view = b[0];
    st.text.x = b[0].x0 + 0x55 - 0x20;
    st.text.y = b[0].y0 + 2;
    print(st, Print::Screen, ULTIMA, &[]);
    let which = if st.transfer.source == source::ULTIMA_IV {
        IV
    } else {
        V
    };
    print(st, Print::Screen, which, &[]);
    st.text.x = st.views.buttons[0].x0 + 0xa8 - 0x22;
    print(st, Print::Screen, ULTIMA_VI, &[]);
    st.views.buttons[1].x1 -= 0xd6;
    let v = st.views.buttons[1];
    st.text.view = v;
    st.text.x = v.x0;
    st.text.y = v.y0 + 2;
    st.text.align = 1;
    print(st, Print::New, LABELS, &[]);
    print(st, Print::More, LABELS_2, &[]);
    st.views.buttons[1].x1 += 0xd6;
    st.text.view = st.views.buttons[1];
    values(st);
    let planes = st.views.screen.planes;
    st.views.v2c64.planes = planes;
    st.views.buttons.iter_mut().for_each(|v| v.planes = planes);
    st.views.draw = st.views.screen;
    st.video.pointer = 1;
    let keys = st.exe.bytes(SHEET_KEYS, 5);
    loop {
        match keys[choose(st, io, &keys).await] {
            0x1b | b'A' => return false,
            b'C' => return true,
            b'N' => {
                let v = st.views.buttons[1];
                st.text.view = v;
                st.text.fg = 0;
                st.text.x = v.x0 + 0x86;
                st.text.y = v.y0 + 2;
                st.video.pointer = 0;
                if let Some(font) = st.font.as_ref() {
                    text::clear_eol(&mut st.video.chip, &st.text, font);
                }
                if let Some(name) = read_line(st, io, 0xd).await {
                    let n = name.len().min(13);
                    st.hero.name[..n].copy_from_slice(&name[..n]);
                    st.hero.name[n] = 0;
                }
                redraw_values(st);
            }
            b'S' => {
                st.video.pointer = 0;
                toggle_sex(st);
                redraw_values(st);
            }
            _ => {}
        }
    }
}

/// Original: u 0x10a81e (transfer a character: the dialog, the sheet, the portrait, the objlist)
/// — 1 when a character was written, else 0
///
/// The menu panel is saved to the spare bitmap and the vellum drawn in it; the dialog repeats
/// until Escape, no character found, or a character accepted on the sheet and the portrait
/// screen. When the objlist cannot be written (a game in progress) passage 0x3e6 says so until
/// a key. At the end the panel comes back (only when the menu is still marked drawn) and the
/// music stops.
pub async fn transfer(st: &mut IntroState, io: &Io) -> u16 {
    music::play_song(st, 4);
    st.views.draw.planes = st.video.other();
    let (d, s) = (st.views.draw, st.views.screen);
    mem::copy_view(&mut st.video.chip, &d, &s);
    st.views.draw = st.views.screen;
    let p = &mut st.views.passage;
    p.y0 = 0;
    p.x0 = 0;
    p.x1 = 0x13f;
    p.y1 = 0x37;
    p.planes = Ptr::at(Region::Work);
    let frame = |v: View| View {
        x1: 0x124,
        x0: 0x1c,
        y0: 0x57,
        y1: 0xbd,
        ..v
    };
    st.views.portrait = frame(st.views.draw);
    st.views.v2c64 = frame(st.views.screen);
    st.views.v17f4 = View {
        planes: st.views.draw.planes,
        ..st.views.panel
    };
    let v = st.views.v17f4;
    mem::fill_view(&mut st.video.chip, &v, 0);
    let pn = st.views.panel;
    let saved = View {
        planes: Ptr::at(Region::Spare),
        x1: pn.x1 - pn.x0,
        x0: 0,
        y0: 0,
        y1: pn.y1 - pn.y0,
    };
    let vellum = st.files_ptr;
    super::load::load_into(&mut st.video.chip, &st.files, b"vellum1.new", vellum);
    let montage = st.files_ptr.add(0x9c40);
    super::load::load_into(&mut st.video.chip, &st.files, b"montage.new", montage);
    st.views.panel.planes = st.video.other();
    let pn = st.views.panel;
    mem::copy_view(&mut st.video.chip, &pn, &saved);
    st.video.fade_out(io, st.exe.palette(5)).await;
    let planes = st.views.screen.planes;
    st.views.panel.planes = planes;
    st.views.v17f4.planes = planes;
    let pn = st.views.panel;
    mem::fill_view(&mut st.video.chip, &pn, 0);
    let v = st.views.v17f4;
    mem::draw_item(&mut st.video.chip, &v, v.x0, v.y0, vellum, 0);
    st.hero.portrait = 0;
    st.transfer.source = source::NONE;
    let mut accepted = false;
    loop {
        let src = ask_source(st, io).await;
        if src == source::NONE {
            break;
        }
        match st.transfer.source {
            source::ULTIMA_IV => {
                read_ultima_iv(st);
                convert_ultima_iv(st);
            }
            source::ULTIMA_V => {
                read_ultima_v(st);
                convert_ultima_v(st);
            }
            _ => {}
        }
        accepted = false;
        st.views.draw.planes = st.video.other();
        let (d, s) = (st.views.draw, st.views.screen);
        mem::copy_view(&mut st.video.chip, &d, &s);
        swap(st, io).await;
        let shown = st.video.other();
        st.views.screen.planes = shown;
        st.views.v2c64.planes = shown;
        st.views.portrait.planes = shown;
        while sheet(st, io).await {
            accepted = choose_portrait(st, io, true).await;
            if accepted {
                break;
            }
        }
        st.views.screen.planes = st.video.current();
        if accepted {
            break;
        }
        st.video.pointer = 0;
    }
    st.video.pointer = 0;
    let mut result = u16::from(accepted);
    if accepted {
        let p = &mut st.views.passage;
        p.y0 = 0;
        p.x0 = 0;
        p.x1 = 0x13f;
        p.y1 = 0x1f;
        p.planes = Ptr::at(Region::Work);
        // 0x107822(8): an empty routine of `u`.
        match super::handoff::write_objlist(st) {
            Ok(bytes) => st.objlist = Some(bytes),
            Err(_) => {
                in_progress(st, io).await;
                result = 0;
            }
        }
    }
    st.views.panel.planes = st.views.screen.planes;
    let pn = st.views.panel;
    mem::fill_view(&mut st.video.chip, &pn, 0);
    // 0x1031b2: an empty routine of `u`.
    if st.menu_drawn != 0 {
        st.video.fade_out(io, st.exe.palette(6)).await;
        let pn = st.views.panel;
        mem::copy_view(&mut st.video.chip, &saved, &pn);
        swap(st, io).await;
    }
    let p = &mut st.views.passage;
    p.y0 = 0;
    p.x0 = 0;
    p.x1 = 0x13f;
    p.y1 = 0x37;
    p.planes = Ptr::at(Region::Work);
    music::play_song(st, 0);
    result
}

/// 0x10aa56..0x10aaf0: the passage that a game is under way (0x3e6) until a key, on a
/// cleared screen; the menu is marked not drawn.
async fn in_progress(st: &mut IntroState, io: &Io) {
    let clear = |st: &mut IntroState| {
        st.views.draw = st.views.screen;
        let d = st.views.draw;
        mem::fill_view(&mut st.video.chip, &d, 0);
    };
    clear(st);
    swap(st, io).await;
    st.views.draw = st.views.screen;
    st.menu_drawn = 0;
    let d = st.views.draw;
    mem::fill_view(&mut st.video.chip, &d, 0);
    passage(st, io, 0x3e6);
    show_passage(st);
    swap(st, io).await;
    st.views.draw = st.views.screen;
    while poll_key(st, io) == 0 {
        st.video.wait_frame(io).await;
    }
    let d = st.views.draw;
    mem::fill_view(&mut st.video.chip, &d, 0);
    swap(st, io).await;
    clear(st);
}

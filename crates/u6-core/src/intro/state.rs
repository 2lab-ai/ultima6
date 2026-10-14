//! The globals of `u` the port keeps (the small-data hunk A4 = 0x116798 and the chip buffers).

use u6_formats::font::WouFont;

use crate::files::Files;

use super::exe::{A4, UExe};
use super::input::Keyboard;
use super::mem::{Ptr, Region, View};
use super::text::TextCtx;
use super::video::Video;

/// The named rectangle records of the data hunk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Views {
    /// Original: A4+0x1784 (the screen view; its planes follow the drawing buffer)
    pub screen: View,
    /// Original: A4+0x1794 (the view of main's text context A4+0x176c)
    pub text: View,
    /// Original: A4+0x17a4 (the menu panel, 0x12..0x12e × 0x50..0xc5)
    pub panel: View,
    /// Original: A4+0x17b4
    pub v17b4: View,
    /// Original: A4+0x17e4 (the view most drawing goes through)
    pub draw: View,
    /// Original: A4+0x17f4
    pub v17f4: View,
    /// Original: A4+0x1804 (the spare bitmap A4+0x2e2e, full screen)
    pub spare: View,
    /// Original: A4+0x25f6 (TV room background)
    pub room: View,
    /// Original: A4+0x2606 (TV room pan layer)
    pub pan: View,
    /// Original: A4+0x2616 (TV room front layer)
    pub front: View,
    /// Original: A4+0x2642 (the TV screen, 0xe5..0x11d × 0x32..0x56)
    pub tv: View,
    /// Original: A4+0x2ac8
    pub v2ac8: View,
    /// Original: A4+0x2ad8 (the gypsy scene saved behind the bottles)
    pub gypsy_bg: View,
    /// Original: A4+0x2b0a (the shimmer rectangle of 0x105a04; its fields A4+0x2b12 .. 0x2b18
    /// are what creation and 0x1059a8 set)
    pub shimmer: View,
    /// Original: A4+0x2c54 (portrait panel)
    pub portrait: View,
    /// Original: A4+0x2c64
    pub v2c64: View,
    /// Original: A4+0x2c8c .. A4+0x2cec (seven views of the portrait screen: text box, then
    /// the buttons)
    pub buttons: [View; 7],
    /// Original: A4+0x2d02 (the passage text bitmap view)
    pub passage: View,
    /// Original: A4+0x2d12 (where 0x10f4da copies the passage text to)
    pub passage_dst: View,
}

/// Original: A4+0x2cfc / A4+0x2cfe / A4+0x2d00 (the scroll frame 0x10f4da draws under a
/// passage: x, y, item of `blocks.new`)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub x: i16,
    pub y: i16,
    pub item: u16,
}

/// The character the creation builds (what 0x105700 writes into the objlist).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hero {
    /// Original: A4+0x2bd8 (14 bytes: the name typed, NUL-terminated; bytes after the NUL keep
    /// what a longer earlier answer left there)
    pub name: [u8; 14],
    /// Original: A4+0x2bee (the sex answer: 'M' / 'F', then "Male" / "Female")
    pub sex: Vec<u8>,
    /// Original: A4+0x2be8 / A4+0x2bea / A4+0x2bec (strength, dexterity, intelligence)
    pub str: i16,
    pub dex: i16,
    pub int: i16,
    /// Original: A4+0x2c08 (experience)
    pub exp: i16,
    /// Original: A4+0x2c0a (level)
    pub level: i16,
    /// Original: A4+0x2c0c (hit points)
    pub hp: i16,
    /// Original: A4+0x2c52 (portrait 0..5 within the sex's set)
    pub portrait: i16,
    /// Original: A4+0x2c12 (the montage item shown: portrait + 0 female / + 6 male)
    pub portrait_item: i16,
    /// Original: A4+0x400 (byte: the objlist byte 0x1BF9; +3 for each Humility answer)
    pub karma: u8,
}

impl Hero {
    /// The name as a C string (up to the NUL).
    pub fn name_str(&self) -> Vec<u8> {
        self.name.iter().copied().take_while(|&b| b != 0).collect()
    }

    /// The sex answer's first byte ('M' / 'F', 0 before an answer).
    pub fn sex_char(&self) -> u8 {
        self.sex.first().copied().unwrap_or(0)
    }
}

/// Every global of `u` the port uses.
#[derive(Clone, Debug)]
pub struct IntroState {
    pub exe: UExe,
    pub video: Video,
    pub keys: Keyboard,
    /// Original: A4+0x1774 (`u6.set`, loaded by 0x10f72c)
    pub font: Option<WouFont>,
    /// Original: A4+0x176c (main's text context)
    pub text: TextCtx,
    pub views: Views,
    pub frame: Frame,
    pub hero: Hero,
    pub tv: super::opening::Tv,
    pub storm: super::storm::Storm,
    pub story: super::story::Story,
    pub gypsy: super::gypsy::Gypsy,
    /// Original: A4+0xb3c (long: state of the C `rand` 0x10fa6c)
    pub rand: u32,
    /// Original: u 0x119996 (long: state of the shimmer generator 0x11999a)
    pub noise: u32,
    /// Original: A4+0x25ea (word: Escape pressed, the opening jumps to its end)
    pub skip: u16,
    /// Original: A4+0x25e8 (word: the menu screen is drawn)
    pub menu_drawn: u16,
    /// Original: A4+0xa2 (word: song playing, 0 = none)
    pub song: u16,
    /// Original: A4+0 (word: the introduction was seen this run)
    pub intro_seen: u16,
    /// Original: A4+0x8d2 (word: the volume the disk prompt 999 asks for)
    pub volume: u16,
    /// Original: A4+0x25c6 (word; 0x106666 fills the screen with colour 0x75 when it is 0x13;
    /// no writer found)
    pub w25c6: u16,
    /// Original: A4+0x182e (byte: draw the A / B buttons of the virtue questions)
    pub buttons_on: u8,
    /// Original: A4+0x2e26 (the `.new` files of a scene)
    pub files_ptr: Ptr,
    /// Original: A4+0x25d4 (`blocks.new`)
    pub blocks: Ptr,
    /// The user's files (the original opens them from the PROGRAM disk).
    pub files: Files,
    /// Port-only: the objlist 0x105700 wrote (the port's files are read-only).
    pub objlist: Option<Vec<u8>>,
}

impl IntroState {
    /// The state `u`'s data hunk starts with, before main (0x1029d0) runs.
    pub fn new(exe: UExe, files: Files) -> Self {
        let video = Video::new(&exe);
        let keys = Keyboard::new(&exe);
        let mover_start = exe.word(A4 + 0xb3a);
        let hero = Hero {
            karma: exe.byte(A4 + 0x400),
            ..Hero::default()
        };
        Self {
            noise: u32::from(exe.word(0x0011_9996)) << 16 | u32::from(exe.word(0x0011_9998)),
            intro_seen: exe.word(A4),
            song: exe.word(A4 + 0xa2),
            buttons_on: exe.byte(A4 + 0x182e),
            volume: exe.word(A4 + 0x8d2),
            w25c6: exe.word(A4 + 0x25c6),
            exe,
            video,
            keys,
            font: None,
            text: TextCtx::default(),
            views: Views::default(),
            frame: Frame::default(),
            hero,
            tv: super::opening::Tv::default(),
            storm: super::storm::Storm::default(),
            story: super::story::Story {
                mover: super::story::Mover::new(mover_start),
                ..super::story::Story::default()
            },
            gypsy: super::gypsy::Gypsy::default(),
            rand: 0,
            skip: 0,
            menu_drawn: 0,
            files_ptr: Ptr::at(Region::Files),
            blocks: Ptr::at(Region::Blocks),
            files,
            objlist: None,
        }
    }

    /// Original: u 0x10fa6c (C runtime `rand`: state × 0x41c64e6d + 0x3039, bits 16..30)
    pub fn rand(&mut self) -> i16 {
        self.rand = self.rand.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        ((self.rand >> 16) & 0x7fff) as i16
    }

    /// Original: u 0x10fa8e (C runtime `srand`: the state = the seed word)
    pub fn srand(&mut self, seed: u16) {
        self.rand = u32::from(seed);
    }

    /// Original: u 0x105608 (`rand() % n`; the 68000 `divs` remainder, 0 when n is 0)
    pub fn random(&mut self, n: i16) -> i16 {
        let r = self.rand();
        if n == 0 { 0 } else { r % n }
    }

    /// Original: u 0x11999a (shimmer generator: shift left, then xor 0x1d872b41 when the shift
    /// carried out a 1 or left 0 (`bhi` skips the xor otherwise); returns the high word)
    pub fn noise_word(&mut self) -> u16 {
        let (v, carry) = (self.noise << 1, self.noise >> 31 != 0);
        self.noise = if !carry && v != 0 { v } else { v ^ 0x1d87_2b41 };
        (self.noise >> 16) as u16
    }
}

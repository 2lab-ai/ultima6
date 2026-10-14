//! Display, palette and the VBlank server of `u` (hunks 14 and 23).
//!
//! `u` double-buffers: of the two screen buffers A4+0x2e32[0..2], one is shown through its
//! copper list while the other is drawn into; 0x113db2 swaps them. Colours are written into the
//! copper lists (0x119c6c), so a palette change, like a buffer swap, shows from the next frame
//! on. The VBlank server 0x1198f8 runs once per frame: it clears the frame-wait flag 0x10fb60
//! (0x115078 waits for that), steps a running fade-in (0x119efa) or fade-out (0x119f90), and
//! every third frame rotates the colour-cycle palette (0x113d80) while 0x119fea is set.
//!
//! The port publishes a frame at every frame wait: the shown buffer read as 5-bit indices,
//! through the colours of that moment, then runs the server's step and awaits the engine's
//! VBlank. One engine tick is one PAL frame.

use crate::engine::Io;

use super::exe::UExe;
use super::mem::{Chip, H, Ptr, Region, W};

/// Display, palette and fade state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Video {
    pub chip: Chip,
    /// Original: A4+0xb4c (word: which of A4+0x2e32[0..2] is drawn into)
    pub cur: u16,
    /// Original: A4+0x2d4e (the buffer the copper list shows)
    pub shown: Region,
    /// Original: A4+0x2d52 (the buffer drawn into)
    pub draw: Region,
    /// The 32 colour registers as the copper lists hold them (12-bit RGB words).
    pub colours: [u16; 32],
    /// Original: u 0x119ff8 (word: palette number installed at the next swap, 0 = none)
    pub queued_palette: u16,
    /// Original: u 0x119e7c (32 words: the palette a fade steps through)
    pub fade: [u16; 32],
    /// Original: u 0x119fec (the target palette of a fade-in)
    pub fade_target: [u16; 32],
    /// Original: u 0x119eeb (byte: fade-in steps left − 1; negative = no fade-in)
    pub fade_in_count: i8,
    /// Original: u 0x119eec (word: the amount still taken off each colour of a fade-in)
    pub fade_in_level: u16,
    /// Original: u 0x119f83 (byte: fade-out steps left − 1; negative = none)
    pub fade_out_count: i8,
    /// Original: u 0x119fea (word: colour cycling on)
    pub cycling: u16,
    /// Original: u 0x10fb62 (word: frames until the next colour-cycle step)
    pub cycle_wait: u16,
    /// Original: A4+0x1392 (the colour-cycle palette; 0x113d80 rotates its words 22..25)
    pub cycle_palette: [u16; 32],
    /// Original: u 0x119ffa (word: mouse pointer shown; the port has no pointer, kept for the
    /// callers that set it)
    pub pointer: u16,
    /// Port-only: frames waited so far (for receipts and tests).
    pub frames: u64,
    /// True while the gradient copper lists (0x11436a / 0x1144fa) show: 0x113df8 picked them,
    /// 0x113db2 the plain ones.
    pub gradient: bool,
    /// The colour changes the gradient lists make down the screen, read from the list at
    /// 0x11436a: (screen row, colour register, 12-bit value), in list order.
    pub gradient_steps: Vec<(i32, usize, u16)>,
}

/// The gradient copper list of buffer 0 (the one of buffer 1, 0x1144fa, is the same but for
/// its bitplane pointers).
const GRADIENT_LIST: u32 = 0x0011_436a;
/// Copper line of screen row 0 (DIWSTRT 0x2c81).
const FIRST_LINE: i32 = 0x2c;

/// The colour moves after the first WAIT of the copper list at `addr` of `u`.
fn gradient_steps(exe: &UExe) -> Vec<(i32, usize, u16)> {
    let mut steps = Vec::new();
    let mut row = None;
    for i in 0..200u32 {
        let (a, b) = (
            exe.word(GRADIENT_LIST + 4 * i),
            exe.word(GRADIENT_LIST + 4 * i + 2),
        );
        if a == 0xffff && b == 0xfffe {
            break;
        }
        if a & 1 != 0 {
            row = Some(i32::from(a >> 8) - FIRST_LINE);
        } else if let Some(r) = row {
            let reg = a & 0x1fe;
            if (0x180..=0x1be).contains(&reg) {
                steps.push((r, usize::from((reg - 0x180) / 2), b));
            }
        }
    }
    steps
}

impl Video {
    /// The state after 0x113bfe: buffer 1 drawn into, buffer 0 shown, all black.
    pub fn new(exe: &UExe) -> Self {
        Self {
            chip: Chip::new(),
            cur: 1,
            shown: Region::Screen0,
            draw: Region::Screen1,
            colours: [0; 32],
            queued_palette: 0,
            fade: [0; 32],
            fade_target: [0; 32],
            fade_in_count: -1,
            fade_in_level: 0,
            fade_out_count: -1,
            cycling: 0,
            cycle_wait: 0,
            cycle_palette: exe.palette(32),
            pointer: 0,
            frames: 0,
            gradient: false,
            gradient_steps: gradient_steps(exe),
        }
    }

    fn buffer(index: u16) -> Region {
        if index & 1 == 0 {
            Region::Screen0
        } else {
            Region::Screen1
        }
    }

    /// Original: u 0x113db2 (display: show the buffer drawn into, draw into the other)
    ///
    /// The shown buffer becomes A4+0x2e32[cur], `cur` flips and the drawing pointer follows;
    /// 0x119b94 installs a queued palette and points the copper at the shown buffer's list.
    /// Returns the new drawing buffer (the original stores it in the caller's view). The wait
    /// for raster line 200 (0x113e66) only lines the write up with the frame; the port's swap
    /// shows from the next frame either way.
    pub fn swap(&mut self, exe: &UExe) -> Ptr {
        self.gradient = false;
        self.swap_lists(exe)
    }

    /// The swap of 0x113db2 / 0x113df8 without the choice of copper lists.
    fn swap_lists(&mut self, exe: &UExe) -> Ptr {
        self.shown = Self::buffer(self.cur);
        self.cur ^= 1;
        self.draw = Self::buffer(self.cur);
        self.install_queued(exe);
        Ptr::at(self.draw)
    }

    /// Original: u 0x113df8 (display: as 0x113db2 with the other pair of copper lists, which
    /// rewrite colour 24 every 6 lines down the first 96 rows: the sky of the woods)
    ///
    /// The port shows the changes by giving the pixels of a changed colour a palette entry of
    /// their own (32 + the change's number) on the rows the change covers.
    pub fn swap_gradient(&mut self, exe: &UExe) -> Ptr {
        self.gradient = true;
        self.swap_lists(exe)
    }

    /// 0x119b94 / 0x119bde: the queued palette goes in with the swap.
    fn install_queued(&mut self, exe: &UExe) {
        if self.queued_palette != 0 {
            self.colours = exe.palette(self.queued_palette);
            self.queued_palette = 0;
        }
    }

    /// Original: u 0x113e8e (display: flip `cur` without showing; returns the new drawing
    /// buffer)
    pub fn flip(&mut self) -> Ptr {
        self.cur ^= 1;
        Ptr::at(Self::buffer(self.cur))
    }

    /// Original: u 0x113ea6 (display: the buffer that is not drawn into)
    pub fn other(&self) -> Ptr {
        Ptr::at(Self::buffer(self.cur ^ 1))
    }

    /// Original: u 0x113ebe (display: the buffer drawn into)
    pub fn current(&self) -> Ptr {
        Ptr::at(Self::buffer(self.cur))
    }

    /// Original: u 0x113fea (display: set palette `n` of the table at A4+0xb92 now, 0x119c6c)
    pub fn set_palette(&mut self, exe: &UExe, n: u16) {
        self.colours = exe.palette(n);
    }

    /// Original: u 0x119ebc (display: start a 16-frame fade from black to `target`: the counter
    /// 0xf runs down to −1)
    pub fn start_fade_in(&mut self, target: [u16; 32]) {
        self.fade_target = target;
        self.fade = [0; 32];
        self.fade_in_level = 0xfff;
        self.fade_in_count = 0x0f;
    }

    /// Original: u 0x119f6c (display: start an 18-frame fade of `from` to black: the counter
    /// 0x11 runs down to −1)
    pub fn start_fade_out(&mut self, from: [u16; 32]) {
        self.fade = from;
        self.fade_out_count = 0x11;
    }

    /// Original: u 0x119efa (VBlank: one fade-in step)
    ///
    /// Each colour is the target less the level, nibble by nibble (clamped at 0); the level
    /// drops by 0x111 and the result goes to the copper lists.
    pub fn fade_in_step(&mut self) {
        if self.fade_in_count < 0 {
            return;
        }
        self.fade_in_count -= 1;
        let level = self.fade_in_level;
        for (out, &c) in self.fade.iter_mut().zip(&self.fade_target) {
            let sub = |mask: u16| (c & mask).saturating_sub(level & mask) & mask;
            *out = sub(0x00f) | sub(0x0f0) | sub(0xf00);
        }
        self.fade_in_level = self.fade_in_level.wrapping_sub(0x111);
        self.colours = self.fade;
    }

    /// Original: u 0x119f90 (VBlank: one fade-out step)
    ///
    /// The fade palette goes to the copper lists, then each nonzero nibble drops by one.
    pub fn fade_out_step(&mut self) {
        if self.fade_out_count < 0 {
            return;
        }
        self.fade_out_count -= 1;
        self.colours = self.fade;
        for c in &mut self.fade {
            let dec =
                |v: u16, mask: u16, one: u16| if v & mask != 0 { (v & mask) - one } else { 0 };
            *c = dec(*c, 0x00f, 0x001) | dec(*c, 0x0f0, 0x010) | dec(*c, 0xf00, 0x100);
        }
    }

    /// Original: u 0x113d80 (VBlank: rotate colours 22..25 of the cycle palette and show it)
    pub fn cycle_step(&mut self) {
        self.cycle_palette[22..26].rotate_left(1);
        self.colours = self.cycle_palette;
    }

    /// Original: u 0x1198f8 (VBlank server "Ultima 6 Int": fades, colour cycling) [partial]
    ///
    /// Not ported: the BEAMCON0 write of 0x119fe8 (PAL/NTSC switch) and the pointer sprite
    /// position 0x119c26 (the port has no mouse).
    pub fn server(&mut self) {
        self.fade_in_step();
        self.fade_out_step();
        if self.cycling != 0 {
            self.cycle_wait = self.cycle_wait.wrapping_sub(1);
            if self.cycle_wait == 0 {
                self.cycle_wait = 3;
                self.cycle_step();
            }
        }
    }

    /// The shown buffer as indices plus the 32 colours, onto the engine's screen.
    pub fn publish(&self, io: &Io) {
        let planes = Ptr::at(self.shown);
        io.draw(|fb| {
            for (i, &c) in self.colours.iter().enumerate() {
                let n = |v: u16| (v & 0xf) as u8 * 17;
                fb.palette[i] = u6_render::Rgb::new(n(c >> 8), n(c >> 4), n(c));
            }
            let steps: &[(i32, usize, u16)] = if self.gradient {
                &self.gradient_steps
            } else {
                &[]
            };
            for (k, &(_, _, c)) in steps.iter().enumerate().take(256 - 32) {
                let n = |v: u16| (v & 0xf) as u8 * 17;
                fb.palette[32 + k] = u6_render::Rgb::new(n(c >> 8), n(c >> 4), n(c));
            }
            let mut over = [None::<u8>; 32];
            let mut next = 0;
            for y in 0..H {
                while next < steps.len().min(256 - 32) && steps[next].0 <= y {
                    over[steps[next].1] = Some(32 + next as u8);
                    next += 1;
                }
                for x in 0..W {
                    let c = self.chip.pixel(planes, x, y);
                    let v = over[usize::from(c)].unwrap_or(c);
                    fb.set(x as usize, y as usize, v);
                }
            }
        });
        io.present();
    }

    /// Original: u 0x115078 (display: wait for the next VBlank)
    ///
    /// Publishes the frame of this moment, runs the server's step and yields to the engine.
    pub async fn wait_frame(&mut self, io: &Io) {
        self.publish(io);
        self.server();
        self.frames += 1;
        io.vblank().await;
    }

    /// Original: u 0x113fb2 (display: fade a palette of the table in and wait until it is done;
    /// 0x119ebc, 0x119eee)
    pub async fn fade_in(&mut self, io: &Io, target: [u16; 32]) {
        self.start_fade_in(target);
        while self.fade_in_count >= 0 {
            self.wait_frame(io).await;
        }
    }

    /// Original: u 0x113fce (display: fade a palette of the table out and wait until it is
    /// done; 0x119f6c, 0x119f84)
    pub async fn fade_out(&mut self, io: &Io, from: [u16; 32]) {
        self.start_fade_out(from);
        while self.fade_out_count >= 0 {
            self.wait_frame(io).await;
        }
    }
}

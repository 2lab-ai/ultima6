//! The separate Amiga `end` program and game's exit-code-2 handoff (spec 16 E1).
//! Original resources stay in the user's files. `u` and `end` share the planar item blitter
//! and proportional text renderer; this module ports `end`'s own layout and scene sequence.
mod assets;
mod passages;
#[cfg(test)]
mod tests;

use crate::intro::mem::{self, Chip, Ptr, Region, View};
use crate::sound::MedCall;
use crate::{Files, Io, State};
use assets::Assets;
pub use assets::Error;
use std::cell::RefCell;

/// Original: 0x13513a, the three decimal argv buffers passed to `end` by the launcher.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Elapsed {
    pub years: i16,
    pub months: i16,
    pub days: i16,
}
impl Elapsed {
    pub fn from_calendar(c: crate::actors::Calendar) -> Self {
        let mut years = c.year.wrapping_sub(161) as i16;
        let mut months = i16::from(c.month) - 7;
        // 0x13515e compares the unsigned original day byte with zero, so the borrow branch
        // is never taken. Preserve that behavior, including negative day differences.
        let days = i16::from(c.day) - 4;
        if months < 0 {
            months += 13;
            years = years.wrapping_sub(1);
        }
        Self {
            years,
            months,
            days,
        }
    }
}
/// Complete preflight before any destructive shrine change; no game state is touched.
pub fn preflight(files: &Files) -> Result<(), Error> {
    Assets::load(files).map(|_| ())
}

/// Original: 0x133d1e (hide the target and refresh the changed map with a dissolve).
pub(crate) fn vanished(s: &mut State, io: &Io) {
    crate::ui::cursor::panel_cursor_in(s, io, 0);
    s.ui.input.targeting = 0;
    crate::gfx::render_window(s, io, false);
    crate::ui::effects::redraw_view(s, io);
    s.gfx.dissolve = 1;
    crate::gfx::render_window(s, io, true);
}
/// Original: 0x13513a (stop game music and exit 2). The async dispatcher unwinds to the
/// launcher without spending movement points or running the next turn.
pub(crate) fn request(s: &mut State) {
    s.main.ending = Some(Elapsed::from_calendar(s.actors.calendar));
    s.main.reading = 0;
    crate::sound::play_song(s, 0);
}
/// Runs the ending in the engine task. A failed preflight produces no frame or mutation.
pub async fn run(state: &RefCell<State>, io: &Io, elapsed: Elapsed) -> Result<(), Error> {
    let assets = Assets::load(io.files())?;
    state.borrow_mut().sound.call(MedCall::NewProcess);
    let mut scene = Scene::new(assets, elapsed);
    scene.main(state, io).await;
    Ok(())
}

struct Scene {
    assets: Assets,
    chip: Chip,
    elapsed: Elapsed,
    layout: passages::Layout,
    passage: u16,
    song: u16,
    mouse_down: bool,
    pending_keys: usize,
    #[cfg(test)]
    receipts: Vec<(u16, u64)>,
}
impl Scene {
    fn new(assets: Assets, elapsed: Elapsed) -> Self {
        let mut chip = Chip::new();
        chip.write(Ptr::at(Region::Files), &assets.pictures);
        Self {
            assets,
            chip,
            elapsed,
            layout: passages::Layout {
                x: 0,
                y: 0,
                item: 0,
                text_y: 0,
                bottom: 0,
                centred: false,
            },
            passage: 0,
            song: 0,
            mouse_down: false,
            pending_keys: 0,
            #[cfg(test)]
            receipts: Vec::new(),
        }
    }
    fn item(&mut self, n: u16, x: i16, y: i16, bottom: i16) {
        let view = View {
            y1: bottom,
            ..View::screen(Ptr::at(Region::Work))
        };
        mem::draw_item(&mut self.chip, &view, x, y, Ptr::at(Region::Files), n);
    }
    /// Original: end 0x1039ce. Only the side strips and rows 81..199 are copied each frame;
    /// the central portal remains on the shown buffer until a dissolve changes it.
    fn copy_scene(&mut self) {
        for (x0, y0, x1, y1) in [(0, 0, 105, 80), (216, 0, 319, 80), (0, 81, 319, 199)] {
            let src = View {
                x0,
                y0,
                x1,
                y1,
                ..View::screen(Ptr::at(Region::Work))
            };
            let dst = View {
                planes: Ptr::at(Region::Screen0),
                ..src
            };
            mem::copy_view(&mut self.chip, &src, &dst);
        }
    }
    fn publish(&self, io: &Io) {
        io.draw(|fb| {
            fb.clear(0);
            for (dst, &c) in fb.palette.iter_mut().zip(&self.assets.palette) {
                *dst = crate::Rgb::new(
                    ((c >> 8) & 15) as u8 * 17,
                    ((c >> 4) & 15) as u8 * 17,
                    (c & 15) as u8 * 17,
                );
            }
            for y in 0..200 {
                for x in 0..320 {
                    fb.set(
                        x,
                        y,
                        self.chip
                            .pixel(Ptr::at(Region::Screen0), x as i32, y as i32),
                    );
                }
            }
        });
        io.present();
    }
    async fn frame(&mut self, io: &Io) {
        self.publish(io);
        #[cfg(test)]
        {
            let mut hash = 0xcbf29ce484222325u64;
            for &b in self.chip.bytes(Ptr::at(Region::Screen0)) {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
            }
            self.receipts.push((self.passage, hash));
        }
        io.vblank().await;
    }
    /// Original: end 0x103908. Either mouse button advances once, on release; any key also
    /// advances. A held mouse yields frames while waiting instead of spinning.
    async fn poll(&mut self, io: &Io) -> bool {
        let p = io.take_pointer();
        if p.buttons != 0 {
            self.mouse_down = true;
        }
        if self.mouse_down {
            while io.take_pointer().buttons != 0 {
                if io.key_ready() {
                    crate::intro::input::try_key(io);
                    self.pending_keys += 1;
                } else {
                    self.frame(io).await;
                }
            }
            self.mouse_down = false;
            return true;
        }
        if self.pending_keys > 0 {
            self.pending_keys -= 1;
            return true;
        }
        if io.key_ready() {
            return crate::intro::input::try_key(io).is_some();
        }
        false
    }
    fn drain(&mut self, io: &Io) {
        self.pending_keys = 0;
        while io.key_ready() {
            crate::intro::input::try_key(io);
        }
    }
    async fn key(&mut self, io: &Io) {
        while !self.poll(io).await {
            self.frame(io).await;
        }
    }
    /// Original: end 0x103940. Its delay routine is an empty RTS; yielding once per poll also
    /// services display/input as the original interrupts do. A key shortens the delay.
    async fn wait(&mut self, io: &Io, n: u16) {
        for _ in 0..n {
            self.frame(io).await;
            if self.poll(io).await {
                break;
            }
        }
    }
    fn song(&mut self, state: &RefCell<State>, n: u16) {
        if n == self.song {
            return;
        }
        let mut s = state.borrow_mut();
        if self.song != 0 {
            s.sound.call(MedCall::Stop);
        }
        if n != 0 {
            s.sound.call(MedCall::Load(
                self.assets.songs[usize::from(n - 10)].clone(),
            ));
            s.sound.call(MedCall::PlayBuffer);
        }
        self.song = n;
    }
    /// Original: end 0x10a1d6. Parameter 30 gives 151 groups, each followed by the VBlank
    /// routine 0x1096d0. The final branch omits the rectangle's x0: it copies screen x=0
    /// rather than x=106 (0x10a3c2..0x10a432). Preserve that original one-pixel behavior.
    async fn dissolve(&mut self, io: &Io) {
        let batch = (self.assets.dissolve.len() - 1) / 151;
        for i in 0..self.assets.dissolve.len() {
            let (x, y) = self.assets.dissolve[i];
            let x = if i + 1 == self.assets.dissolve.len() {
                0
            } else {
                i32::from(x) + 106
            };
            let y = i32::from(y);
            let c = self.chip.pixel(Ptr::at(Region::Work), x, y);
            self.chip.set_pixel(Ptr::at(Region::Screen0), x, y, c);
            if (i + 1) % batch == 0 {
                self.frame(io).await;
            }
        }
        self.frame(io).await;
    }
    /// Original: end 0x103a1a (argc check replaced by the launcher's typed elapsed values).
    async fn main(&mut self, state: &RefCell<State>, io: &Io) {
        state.borrow_mut().main.program = crate::main_loop::Program::End;
        // End's copper setup 0x10d342 points at the empty sprite at 0x10b36a. The game's
        // hardware sprite must not survive this process boundary in the host compositor.
        state.borrow_mut().ui.pointer.shown = false;
        io.reset_src_plane();
        self.song(state, 10);
        self.item(4, 106, 0, 199);
        self.item(7, 0, 0, 199);
        self.item(5, 73, 87, 199);
        self.item(6, 218, 87, 199);
        self.drain(io);
        self.passage(0);
        let src = View::screen(Ptr::at(Region::Work));
        let dst = View::screen(Ptr::at(Region::Screen0));
        mem::copy_view(&mut self.chip, &src, &dst);
        self.compose();
        self.wait(io, 30).await;
        self.copy_scene();
        let mut y = 125;
        loop {
            self.item(7, 0, 0, 199);
            self.item(3, 9, y, 125);
            self.item(5, 73, 87, 199);
            self.item(6, 218, 87, 199);
            self.compose();
            self.copy_scene();
            self.frame(io).await;
            let done = y == -12;
            if self.poll(io).await {
                y = -9;
            }
            y = (y - 3).max(-12);
            if done {
                break;
            }
        }
        self.drain(io);
        self.passage(1);
        self.key(io).await;
        let mut phase = 0u16;
        let mut dissolved = 0;
        loop {
            self.item(7, 0, 0, 199);
            let mut moving = false;
            if phase < 2 && y < 125 {
                self.item(3, 9, y, 125);
                if phase == 1 {
                    y += 3;
                    moving = true;
                }
            }
            if phase == 6 || phase == 7 {
                moving = true;
                self.item(3, 230, y, 125);
                y = (y - 3).max(-12);
            }
            if phase == 8 && y < 125 {
                moving = true;
                y += 3;
                self.item(3, 230, y, 125);
            }
            if (7..=10).contains(&phase) {
                self.item(13, 175, 0, 199);
            }
            if phase < 11 {
                self.item(6, 218, 87, 199);
            } else {
                self.item(14, 220, 0, 199);
            }
            if phase < 4 {
                self.item(8, 0, 0, 199);
                self.item(5, 73, 87, 199);
            } else {
                self.item(9, 0, 0, 199);
            }
            if phase != 14 {
                self.compose();
            }
            self.copy_scene();
            self.frame(io).await;
            match phase {
                4 if dissolved != 1 => {
                    self.wait(io, 5).await;
                    self.item(0, 106, 0, 93);
                    self.dissolve(io).await;
                    self.item(1, 151, 29, 93);
                    self.dissolve(io).await;
                    self.drain(io);
                    dissolved = 1;
                }
                12 if dissolved != 2 => {
                    self.wait(io, 5).await;
                    self.song(state, 12);
                    self.item(2, 133, 23, 93);
                    self.dissolve(io).await;
                    self.drain(io);
                    dissolved = 2;
                }
                14 => {
                    self.wait(io, 30).await;
                    self.item(0, 106, 0, 93);
                    self.dissolve(io).await;
                    self.item(4, 106, 0, 93);
                    self.dissolve(io).await;
                    self.key(io).await;
                    break;
                }
                _ => {}
            }
            let advance = if moving {
                self.poll(io).await
            } else {
                self.key(io).await;
                true
            };
            if advance {
                phase += 1;
                self.drain(io);
                match phase {
                    6 => {
                        self.passage(7);
                        self.song(state, 11);
                        y = 125;
                    }
                    7 => {
                        self.passage(8);
                        y = -12;
                    }
                    14 => {}
                    _ => self.passage(phase + 1),
                }
            }
        }
        mem::fill_view(&mut self.chip, &View::screen(Ptr::at(Region::Work)), 0);
        self.passage(15);
        self.compose();
        mem::copy_view(
            &mut self.chip,
            &View::screen(Ptr::at(Region::Work)),
            &View::screen(Ptr::at(Region::Screen0)),
        );
        self.drain(io);
        self.frame(io).await;
        self.key(io).await;
        self.song(state, 0);
    }
}

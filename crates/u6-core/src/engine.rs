//! The executor: the original's blocking program as one retained async future (spec 03 §1).
//!
//! The 68000 `game` blocks inside its command loop (key wait 0x10790a → 0x106bbc, delays, disk
//! prompts). The port keeps that shape: game code is `async fn`, and every place the original
//! waits on input, time or the display is an `.await` on [`Io`]. [`Engine::tick`] polls the single
//! game future exactly once per host tick, also while it waits, with a no-op waker (valid only
//! because polling is unconditional). No threads, no runtime, wasm-safe.
//!
//! Rules every game function follows:
//!
//! - No `Ref`/`RefMut` into [`State`] or [`Io`] lives across an `.await`: copy values out, reborrow
//!   after. [`Io::draw`] takes a closure so a screen borrow cannot escape.
//! - Every potentially unbounded loop reaches a real yield ([`Io::vblank`], [`Io::next_key`] on an
//!   empty queue, or [`Io::sleep`] with a later deadline).
//! - Display time is not game time: ticks advance the millisecond clock and the display
//!   generation, never game turns or the calendar.
//! - Recursive async cycles put `Box::pin` on the cycle edge.

use std::cell::{Cell, OnceCell, Ref, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::files::Files;
use crate::state::{LoadError, State};
use crate::stub::{self, StubLog};
use crate::{Framebuffer, main_loop};

/// Logical keys. Platforms map their key codes onto these; the original's raw-key → game-code
/// table (0x151d00) is ported on top of them by `ui/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Space,
    Backspace,
    /// A printable character as typed (letters keep their case).
    Char(char),
}

/// One input event, queued by the platform and consumed in order by the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputEvent {
    /// A key press (auto-repeat arrives as further presses).
    Key(Key),
    /// The pointer (mouse) at screen pixel (x, y), 0..=319 × 0..=199 (the platform scales from
    /// its window; values outside are clamped), with `buttons` held: bit 0 left, bit 1 right
    /// ([`POINTER_LEFT`], [`POINTER_RIGHT`]).
    Pointer { x: i16, y: i16, buttons: u8 },
}

/// Left button bit of [`InputEvent::Pointer`] (the original's button word 0x161bd4, bit 0).
pub const POINTER_LEFT: u8 = 1;
/// Right button bit (0x161bd4 bit 1).
pub const POINTER_RIGHT: u8 = 2;

/// [`Io::keys_owned`] bit: the game takes `-` now.
pub const OWN_MINUS: u8 = 1;
/// [`Io::keys_owned`] bit: the game takes `+` now.
pub const OWN_PLUS: u8 = 2;
/// [`Io::keys_owned`] bit: the game takes `=` now.
pub const OWN_EQUALS: u8 = 4;
/// All three [`Io::keys_owned`] bits (a line being typed).
pub const OWN_ALL: u8 = OWN_MINUS | OWN_PLUS | OWN_EQUALS;
/// Number of [`Io::keys_owned`] bits.
const OWN_KEYS: usize = 3;

/// The pointer as the input handler keeps it: position 0x1623b8 / 0x1623ba (pixels, clamped to
/// the screen) and the button word 0x161bd4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer {
    pub x: i16,
    pub y: i16,
    pub buttons: u8,
}

impl Pointer {
    /// Replaces: 0x151b08 (input.device handler, mouse class: the move is added to the position
    /// and clamped to 0..=319 / 0..=199; codes 0x68 / 0xe8 and 0x69 / 0xe9 set and clear
    /// button bits 0 and 1). Platforms report absolute positions, so the position is taken.
    fn apply(x: i16, y: i16, buttons: u8) -> Pointer {
        Pointer {
            x: x.clamp(0, 0x13f),
            y: y.clamp(0, 199),
            buttons: buttons & (POINTER_LEFT | POINTER_RIGHT),
        }
    }
}

/// The game's view of the platform: input queue, millisecond clock, display generation, two
/// screen buffers and the file layer. Nothing else in the core touches time, input or files.
pub struct Io {
    events: RefCell<VecDeque<InputEvent>>,
    pointer: Cell<Pointer>,
    now_ms: Cell<u64>,
    generation: Cell<u64>,
    back: RefCell<Framebuffer>,
    front: RefCell<Framebuffer>,
    front_version: Cell<u64>,
    /// The screen as the engine last published it (what the player sees, without the pointer).
    published: RefCell<Framebuffer>,
    files: Files,
    /// The layout of `game` in `files`, parsed at the first [`Io::game_image`] (`files` never
    /// changes, so it stays valid for the run).
    image: OnceCell<Option<crate::image::ImageLayout>>,
    /// Port-only, no original address: per key of [`OWN_MINUS`] / [`OWN_PLUS`] / [`OWN_EQUALS`]
    /// (bit order), the claims on it that live now ([`Io::own_keys`]): the keys the game takes
    /// where its state is not in [`State`] (`u`'s).
    owned_keys: Cell<[u32; OWN_KEYS]>,
}

impl Io {
    fn new(files: Files) -> Self {
        Self {
            events: RefCell::new(VecDeque::new()),
            pointer: Cell::new(Pointer::default()),
            now_ms: Cell::new(0),
            generation: Cell::new(0),
            back: RefCell::new(Framebuffer::new()),
            front: RefCell::new(Framebuffer::new()),
            front_version: Cell::new(0),
            published: RefCell::new(Framebuffer::new()),
            files,
            image: OnceCell::new(),
            owned_keys: Cell::new([0; OWN_KEYS]),
        }
    }

    /// Milliseconds of accumulated tick time (monotonic).
    pub fn now_ms(&self) -> u64 {
        self.now_ms.get()
    }

    /// Display generation: bumped once per tick, after the poll.
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Events queued and not yet consumed.
    pub fn pending_events(&self) -> usize {
        self.events.borrow().len()
    }

    /// The screen as it was last published (the picture on display, before this poll's draws).
    pub fn published<R>(&self, f: impl FnOnce(&Framebuffer) -> R) -> R {
        f(&self.published.borrow())
    }

    /// The pointer where the input handler has it: the last queued pointer event (the handler
    /// moves the position as events come, 0x151b08), else the last one taken. For the display
    /// (the VBlank server's sprite) and the platforms (a button held paces the steps at 1×,
    /// spec 06); the game's own sample is [`Io::take_pointer`]'s.
    pub fn handler_pointer(&self) -> Pointer {
        let events = self.events.borrow();
        let last = events.iter().rev().find_map(|e| match *e {
            InputEvent::Pointer { x, y, buttons } => Some(Pointer::apply(x, y, buttons)),
            InputEvent::Key(_) => None,
        });
        last.unwrap_or(self.pointer.get())
    }

    /// The game files (replaces the dos.library file layer).
    pub fn files(&self) -> &Files {
        &self.files
    }

    /// The user's `game` mapped by image address ([`crate::image::GameImage`]), parsed once:
    /// the strings and tables the handlers read on every print come through here. `None` when
    /// the file is missing or not a hunk file.
    pub fn game_image(&self) -> Option<crate::image::GameImage<'_>> {
        let game = self.files.get("game")?;
        let layout = self
            .image
            .get_or_init(|| {
                crate::image::GameImage::parse(game)
                    .ok()
                    .map(crate::image::GameImage::into_layout)
            })
            .as_ref()?;
        Some(crate::image::GameImage::with_layout(game, layout))
    }

    /// Replaces: 0x151b08 (input.device handler; the platform queues events instead)
    ///
    /// Resolves with the next queued key, in arrival order; pending while the queue is empty.
    /// Pointer events before it are applied to the pointer on the way (as the handler would),
    /// for the waits that read only keys.
    pub fn next_key(&self) -> NextKey<'_> {
        NextKey { io: self }
    }

    /// True when the next queued event is a key (a pointer event before it is taken first, by
    /// [`Io::take_pointer`]).
    pub fn key_ready(&self) -> bool {
        matches!(self.events.borrow().front(), Some(InputEvent::Key(_)))
    }

    /// The pointer as the handler last left it (no event is taken).
    pub fn pointer(&self) -> Pointer {
        self.pointer.get()
    }

    /// Takes the pointer events at the front of the queue up to the next change of the buttons:
    /// the first one, then those after it that keep its buttons (moves). So each call delivers
    /// at most one press or release, at the position the moves after it reached; a key in front
    /// stays queued and holds the pointer events behind it (input is consumed in order).
    /// Returns the pointer as it then is.
    pub fn take_pointer(&self) -> Pointer {
        let mut events = self.events.borrow_mut();
        let mut first = true;
        while let Some(&InputEvent::Pointer { x, y, buttons }) = events.front() {
            let p = Pointer::apply(x, y, buttons);
            if !first && p.buttons != self.pointer.get().buttons {
                break;
            }
            self.pointer.set(p);
            events.pop_front();
            first = false;
        }
        self.pointer.get()
    }

    /// As [`Io::take_pointer`], taking at most the first `n` queued events (none when `n` is 0):
    /// for a sample as of a time before the latest events came.
    pub fn take_pointer_upto(&self, n: usize) -> Pointer {
        let mut events = self.events.borrow_mut();
        let mut taken = 0;
        while let Some(&InputEvent::Pointer { x, y, buttons }) = events.front() {
            let p = Pointer::apply(x, y, buttons);
            if taken == n || (taken > 0 && p.buttons != self.pointer.get().buttons) {
                break;
            }
            self.pointer.set(p);
            events.pop_front();
            taken += 1;
        }
        self.pointer.get()
    }

    /// Resolves once the tick clock reaches now + `ms` (deadline fixed at the call).
    pub fn sleep(&self, ms: u32) -> Sleep<'_> {
        Sleep {
            io: self,
            deadline: self.now_ms() + u64::from(ms),
        }
    }

    /// Replaces: 0x1622be (wait for the vertical blank: set the flag 0x1624b0 the server clears)
    ///
    /// Resolves at a later display generation than the current one, so never within the tick
    /// that created it: whatever was presented before the await is published first.
    pub fn vblank(&self) -> VBlank<'_> {
        VBlank {
            io: self,
            target: self.generation() + 1,
        }
    }

    /// Draws on the displayed screen (the front buffer, 0x161bde: the text layer, panels and
    /// 0x153002 draw here). The borrow ends with the closure, so it cannot cross an await.
    pub fn draw<R>(&self, f: impl FnOnce(&mut Framebuffer) -> R) -> R {
        f(&mut self.front.borrow_mut())
    }

    /// Draws into the other buffer (the back buffer, 0x161be2: the map window is composed
    /// here by 0x152ffa and copied to the screen by 0x151f5a / 0x1520e2 / 0x151fe6).
    pub fn draw_back<R>(&self, f: impl FnOnce(&mut Framebuffer) -> R) -> R {
        f(&mut self.back.borrow_mut())
    }

    /// Both buffers at once (back read-write, front read-write), for the copies between them.
    pub fn with_buffers<R>(&self, f: impl FnOnce(&mut Framebuffer, &mut Framebuffer) -> R) -> R {
        f(&mut self.back.borrow_mut(), &mut self.front.borrow_mut())
    }

    /// Port-only (spec 06 §Design 6): the keys among [`OWN_MINUS`], [`OWN_PLUS`] and
    /// [`OWN_EQUALS`] that game code waiting now has claimed with [`Io::own_keys`], so the page
    /// leaves them to it instead of changing the speed.
    pub fn keys_owned(&self) -> u8 {
        let counts = self.owned_keys.get();
        (0..OWN_KEYS)
            .filter(|&i| counts[i] != 0)
            .fold(0, |bits, i| bits | 1 << i)
    }

    /// Port-only: claims `bits` of [`Io::keys_owned`] until the guard drops (the end of the wait
    /// that reads them, an early return, the future dropped, an unwind). Claims count per key,
    /// so guards may drop in any order: a key stays owned while any claim on it lives.
    pub fn own_keys(&self, bits: u8) -> OwnedKeys<'_> {
        self.count_claims(bits, true);
        OwnedKeys { io: self, bits }
    }

    /// Adds (`add`) or takes off one claim on each key of `bits`.
    fn count_claims(&self, bits: u8, add: bool) {
        let mut counts = self.owned_keys.get();
        for (i, n) in counts.iter_mut().enumerate() {
            if bits & 1 << i != 0 {
                *n = if add { *n + 1 } else { *n - 1 };
            }
        }
        self.owned_keys.set(counts);
    }

    /// Marks the screen (front buffer) as shown: the engine publishes it after this poll. The
    /// Amiga displays its screen buffer continuously; the port publishes at the points where
    /// the original waits (key poll, VBlank), so a frame is never shown half drawn.
    pub fn present(&self) {
        self.front_version.set(self.front_version.get() + 1);
    }
}

/// Keys claimed by [`Io::own_keys`], given back on drop.
#[must_use = "the keys are given back as soon as the guard drops"]
pub struct OwnedKeys<'a> {
    io: &'a Io,
    bits: u8,
}

impl Drop for OwnedKeys<'_> {
    fn drop(&mut self) {
        self.io.count_claims(self.bits, false);
    }
}

thread_local! {
    /// The `Io` of the engine that is polling (set by [`Engine::tick`] around its poll).
    static CURRENT: RefCell<Option<Rc<Io>>> = const { RefCell::new(None) };
}

/// Runs `f` with the `Io` of the engine that is polling, or returns `None` outside a poll.
///
/// For synchronous game code that the original lets draw or open a file in the middle of a
/// routine (a party move redraws the view between steps, 0x102ef0): it runs inside the one
/// game poll, so it reaches the same buffers the async code draws on, in call order. It never
/// waits; anything that waits (a more prompt, a key) stays on the async path.
pub fn with_current_io<R>(f: impl FnOnce(&Io) -> R) -> Option<R> {
    let io = CURRENT.with(|c| c.borrow().clone())?;
    Some(f(&io))
}

/// Runs `f` with `io` as the current `Io`, restoring the previous one afterwards.
fn scoped_io<R>(io: &Rc<Io>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Rc<Io>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = prev);
        }
    }
    let prev = CURRENT.with(|c| c.borrow_mut().replace(Rc::clone(io)));
    let _restore = Restore(prev);
    f()
}

/// Future of [`Io::next_key`].
pub struct NextKey<'a> {
    io: &'a Io,
}

impl Future for NextKey<'_> {
    type Output = Key;
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Key> {
        let mut events = self.io.events.borrow_mut();
        loop {
            match events.pop_front() {
                Some(InputEvent::Key(key)) => return Poll::Ready(key),
                Some(InputEvent::Pointer { x, y, buttons }) => {
                    self.io.pointer.set(Pointer::apply(x, y, buttons));
                }
                None => return Poll::Pending,
            }
        }
    }
}

/// Future of [`Io::sleep`].
pub struct Sleep<'a> {
    io: &'a Io,
    deadline: u64,
}

impl Future for Sleep<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.io.now_ms() >= self.deadline {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Future of [`Io::vblank`].
pub struct VBlank<'a> {
    io: &'a Io,
    target: u64,
}

impl Future for VBlank<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.io.generation() >= self.target {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Why the engine could not start.
#[derive(Debug)]
pub enum BootError {
    /// A game file is missing or does not parse.
    Load(LoadError),
}

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BootError::Load(e) => write!(f, "cannot load the game: {e}"),
        }
    }
}

impl std::error::Error for BootError {}

impl From<LoadError> for BootError {
    fn from(e: LoadError) -> Self {
        BootError::Load(e)
    }
}

/// Keys the input handler's ring holds (0x14f0ac: 50 slots at A4+0x64f6, one kept free so a
/// full ring differs from an empty one); a key that finds it full is dropped. The port counts
/// presses ([`Key`] is one press): exact for auto-repeat, which sends presses only, but about
/// twice the original's type-ahead for keys tapped one by one, whose releases take a slot of
/// the ring there too.
const KEY_RING: usize = 49;

/// The game future type the engine retains.
type Task = Pin<Box<dyn Future<Output = ()>>>;

/// The running game: state, platform view, the one retained game future, and the published
/// frame.
pub struct Engine {
    state: Rc<RefCell<State>>,
    io: Rc<Io>,
    task: Task,
    finished: bool,
    ticks: u64,
    published_version: u64,
    /// The published frame with the pointer sprite over it (the frame the platform shows).
    shown: Framebuffer,
    sprite: Option<crate::ui::pointer::SpriteFrame>,
    stubs: Rc<StubLog>,
}

impl Engine {
    /// Boots the game from `files` with RNG seed word 0 (the image's own initial state); the
    /// launcher's sequence, as [`Engine::with_seed`].
    pub fn new(files: Files) -> Result<Engine, BootError> {
        Self::with_seed(files, 0)
    }

    /// Replaces: 0x14ef06 (system setup: memory, libraries, copper, VBlank server, input)
    ///
    /// Boots with an explicit RNG seed. The original seeds from the low word of `time()`
    /// (see [`crate::rng`]); platforms pass a clock value for variety or a constant for replays.
    /// The task is the launcher's sequence ([`main_loop::run`]): `u` first, then `game`.
    pub fn with_seed(files: Files, seed: u16) -> Result<Engine, BootError> {
        Self::boot(files, seed, main_loop::run)
    }

    /// As [`Engine::with_seed`], but the task starts `game` at once, without the launcher and
    /// `u` ([`main_loop::game_first`]): the platforms' development entry (the oracle's
    /// game-first disks start the same way) and tests of the game.
    pub fn game_first(files: Files, seed: u16) -> Result<Engine, BootError> {
        Self::boot(files, seed, main_loop::game_first)
    }

    fn boot<G, F>(files: Files, seed: u16, game: G) -> Result<Engine, BootError>
    where
        G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
        F: Future<Output = ()> + 'static,
    {
        let stubs = Rc::new(StubLog::default());
        let mut state = stub::scoped(&stubs, || State::new_game(&files))?;
        state.rng.seed(seed);
        Ok(Self::build(state, files, stubs, game))
    }

    /// An engine around any game future (tests, tools). `game` receives the shared state and
    /// the platform view and is polled once per [`Engine::tick`].
    pub fn with_game<G, F>(state: State, files: Files, game: G) -> Engine
    where
        G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
        F: Future<Output = ()> + 'static,
    {
        Self::build(state, files, Rc::new(StubLog::default()), game)
    }

    fn build<G, F>(state: State, files: Files, stubs: Rc<StubLog>, game: G) -> Engine
    where
        G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
        F: Future<Output = ()> + 'static,
    {
        let state = Rc::new(RefCell::new(state));
        let io = Rc::new(Io::new(files));
        let task: Task = Box::pin(game(Rc::clone(&state), Rc::clone(&io)));
        Engine {
            state,
            io,
            task,
            finished: false,
            ticks: 0,
            published_version: 0,
            shown: Framebuffer::new(),
            sprite: None,
            stubs,
        }
    }

    /// One host tick: queue `input` (keys past the handler's ring, [`KEY_RING`], are dropped),
    /// advance the clock by `elapsed_ms`, poll the game once (also while it waits; never after it
    /// finished), bump the display generation, publish the front buffer if the game presented,
    /// then the vertical-blank work of the display (the pointer sprite, at the handler's
    /// position, over the published frame, [`crate::ui::pointer::vblank`]).
    pub fn tick(&mut self, input: &[InputEvent], elapsed_ms: u32) {
        {
            let mut events = self.io.events.borrow_mut();
            let mut keys = events
                .iter()
                .filter(|e| matches!(e, InputEvent::Key(_)))
                .count();
            for &e in input {
                if matches!(e, InputEvent::Key(_)) {
                    if keys >= KEY_RING {
                        continue;
                    }
                    keys += 1;
                }
                events.push_back(e);
            }
        }
        self.io.now_ms.set(self.io.now_ms() + u64::from(elapsed_ms));
        if !self.finished {
            let mut cx = Context::from_waker(Waker::noop());
            let task = &mut self.task;
            let io = &self.io;
            let poll = || scoped_io(io, || task.as_mut().poll(&mut cx));
            if stub::scoped(&self.stubs, poll).is_ready() {
                self.finished = true;
            }
        }
        self.ticks += 1;
        self.io.generation.set(self.io.generation() + 1);
        let version = self.io.front_version.get();
        let presented = version != self.published_version;
        if presented {
            self.io
                .published
                .borrow_mut()
                .clone_from(&self.io.front.borrow());
            self.published_version = version;
        }
        let at = self.io.handler_pointer();
        let sprite = crate::ui::pointer::vblank(&mut self.state.borrow_mut(), at);
        if presented || sprite != self.sprite || self.ticks == 1 {
            self.shown.clone_from(&self.io.published.borrow());
            if let Some(sp) = &sprite {
                sp.draw(&mut self.shown);
            }
            self.sprite = sprite;
        }
    }

    /// The last published frame (320×200 indices plus the palette) with the pointer sprite.
    pub fn frame(&self) -> &Framebuffer {
        &self.shown
    }

    /// True once the game future returned (it is never polled again).
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Ticks run so far.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// The game state, between ticks (drop the guard before the next tick).
    pub fn state(&self) -> Ref<'_, State> {
        self.state.borrow()
    }

    /// The platform view (clock, generation, queue length).
    pub fn io(&self) -> &Io {
        &self.io
    }

    /// Every `todo_stub!` id hit so far, with hit counts.
    pub fn stub_hits(&self) -> BTreeMap<&'static str, u64> {
        self.stubs.hits()
    }

    /// Log lines (first hit of each stub) since the previous call; the platform prints them.
    pub fn take_log(&mut self) -> Vec<String> {
        self.stubs.take_lines()
    }

    /// Port-only (spec 06 §Design 6): which of `-`, `+` and `=` ([`OWN_MINUS`], [`OWN_PLUS`],
    /// [`OWN_EQUALS`]) the game takes now, as of the last tick: those `u` claimed on the `Io`
    /// ([`Io::keys_owned`]) and those `game`'s state reads ([`crate::ui::target::keys_owned`]).
    /// None once the game finished.
    pub fn keys_owned(&self) -> u8 {
        if self.finished {
            return 0;
        }
        self.io.keys_owned() | crate::ui::target::keys_owned(&self.state.borrow())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> InputEvent {
        InputEvent::Key(Key::Char(c))
    }

    fn engine_with<G, F>(game: G) -> Engine
    where
        G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
        F: Future<Output = ()> + 'static,
    {
        Engine::with_game(State::empty(), Files::default(), game)
    }

    /// Awaits a key, sleeps 100 ms, repeats; logs each step with the tick clock.
    fn key_sleep_game() -> (Rc<RefCell<Vec<String>>>, Engine) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let out = Rc::clone(&log);
        let engine = engine_with(move |_state, io| async move {
            loop {
                let k = io.next_key().await;
                out.borrow_mut().push(format!("{k:?}@{}", io.now_ms()));
                io.sleep(100).await;
                out.borrow_mut().push(format!("woke@{}", io.now_ms()));
            }
        });
        (log, engine)
    }

    fn run_script(engine: &mut Engine) {
        engine.tick(&[], 1000);
        engine.tick(&[key('a')], 10);
        engine.tick(&[], 89);
        engine.tick(&[], 11);
        engine.tick(&[], 5000);
        engine.tick(&[key('b'), key('c')], 0);
        engine.tick(&[], 100);
        engine.tick(&[], 100);
    }

    #[test]
    fn game_advances_only_on_input_and_time() {
        let (log, mut e) = key_sleep_game();
        e.tick(&[], 1000);
        assert!(log.borrow().is_empty(), "no key, no progress");
        e.tick(&[key('a')], 10);
        assert_eq!(*log.borrow(), ["Char('a')@1010"]);
        // The deadline is fixed when the sleep starts: 1010 + 100.
        e.tick(&[], 89);
        assert_eq!(log.borrow().len(), 1, "sleep not over at 1099");
        e.tick(&[], 10);
        assert_eq!(log.borrow().len(), 1, "sleep not over at 1109");
        e.tick(&[], 1);
        assert_eq!(log.borrow().last().unwrap(), "woke@1110");
        e.tick(&[], 5000);
        assert_eq!(log.borrow().len(), 2, "waiting for a key again");
        e.tick(&[key('b'), key('c')], 0);
        // 'b' is taken at once; 'c' stays queued behind the sleep.
        assert_eq!(log.borrow().last().unwrap(), "Char('b')@6110");
        assert_eq!(e.io().pending_events(), 1);
        e.tick(&[], 100);
        // The sleep ends and the already-queued 'c' is consumed in the same poll.
        assert_eq!(log.borrow()[3..], ["woke@6210", "Char('c')@6210"]);
        assert!(!e.finished());
    }

    #[test]
    fn same_inputs_give_the_same_run() {
        let (a, mut ea) = key_sleep_game();
        let (b, mut eb) = key_sleep_game();
        run_script(&mut ea);
        run_script(&mut eb);
        assert_eq!(*a.borrow(), *b.borrow());
        assert_eq!(a.borrow().len(), 6);
    }

    #[test]
    fn vblank_completes_only_at_a_later_generation() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let out = Rc::clone(&seen);
        let mut e = engine_with(move |_state, io| async move {
            loop {
                out.borrow_mut().push(io.generation());
                io.vblank().await;
            }
        });
        for _ in 0..4 {
            e.tick(&[], 0);
        }
        // One loop iteration per tick, never two in one poll.
        assert_eq!(*seen.borrow(), [0, 1, 2, 3]);
        assert_eq!(e.io().generation(), 4);
    }

    /// Panics if polled after it returned Ready.
    struct OnceFuture {
        done: bool,
    }

    impl Future for OnceFuture {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            assert!(!self.done, "polled after completion");
            self.done = true;
            Poll::Ready(())
        }
    }

    /// An engine whose game only sleeps (it takes no input), with the pointer tables of one
    /// shape shown.
    fn idle_engine() -> Engine {
        let mut s = State::empty();
        s.ui.pointer =
            crate::ui::pointer::PointerState::from_tables(crate::ui::pointer::PointerTables {
                shapes: vec![crate::ui::pointer::Shape {
                    dx: -1,
                    dy: -2,
                    rows: vec![[1; 16]],
                }],
                shown: true,
                ..Default::default()
            });
        Engine::with_game(s, Files::default(), |_state, io| async move {
            io.sleep(u32::MAX).await;
        })
    }

    #[test]
    fn keys_beyond_the_handlers_ring_are_dropped() {
        // 0x14f0ac puts a key in the 50-slot ring at A4+0x64f6 only when the next slot is not
        // the reader's (A4-0x35cd / -0x35ce): 49 wait at most, one past them is lost. The port
        // counts presses (a release takes a slot there too: auto-repeat is exact, taps get
        // about twice the room). Pointer events are no keys.
        let mut e = idle_engine();
        let keys: Vec<InputEvent> = (0..60).map(|_| key('a')).collect();
        e.tick(&keys, 16);
        e.tick(
            &[
                InputEvent::Pointer {
                    x: 5,
                    y: 5,
                    buttons: 0,
                },
                key('b'),
            ],
            16,
        );
        let queued: Vec<InputEvent> = e.io.events.borrow().iter().copied().collect();
        let n = queued
            .iter()
            .filter(|ev| matches!(ev, InputEvent::Key(_)))
            .count();
        println!(
            "60 keys then 1 queued: {n} keys wait, {} events",
            queued.len()
        );
        assert_eq!(n, 49, "the ring's room");
        assert!(
            !queued.contains(&key('b')),
            "the key past a full ring is lost"
        );
        assert_eq!(queued.len(), 50, "the pointer event is kept");
    }

    #[test]
    fn the_sprite_follows_the_handler_while_the_game_waits() {
        // The VBlank server moves the sprite to the handler's position (0x1622d6 reads
        // 0x1623b8 / 0x1623ba) whether or not the game samples it: while the game waits, the
        // shown sprite follows the queued moves; the pointer the game takes stays where it was.
        let mut e = idle_engine();
        e.tick(&[], 16);
        e.tick(
            &[InputEvent::Pointer {
                x: 100,
                y: 50,
                buttons: 0,
            }],
            16,
        );
        let at = e.sprite.as_ref().map(|sp| (sp.x, sp.y));
        println!(
            "sprite at {at:?}, the game's pointer {:?}",
            e.io().pointer()
        );
        assert_eq!(at, Some((99, 48)), "the sprite at the queued position");
        assert_eq!(
            e.io().pointer(),
            Pointer::default(),
            "the game took nothing"
        );
        assert_eq!(e.frame().get(99, 48), Some(17), "drawn there");
    }

    #[test]
    fn claimed_keys_last_while_their_guard_does_and_nest() {
        let mut e = engine_with(|_state, io| async move {
            let plus = io.own_keys(OWN_PLUS);
            io.vblank().await;
            {
                let _inner = io.own_keys(OWN_MINUS | OWN_EQUALS);
                io.vblank().await;
            }
            io.vblank().await;
            drop(plus);
            io.vblank().await;
            let _last = io.own_keys(OWN_ALL);
        });
        let mut seen = Vec::new();
        for _ in 0..5 {
            e.tick(&[], 20);
            seen.push(e.keys_owned());
        }
        assert_eq!(
            seen,
            [OWN_PLUS, OWN_ALL, OWN_PLUS, 0, 0],
            "each given back at its drop"
        );
        assert!(e.finished());
        assert_eq!(
            e.io().keys_owned(),
            0,
            "a claim at the end ended with the game"
        );
    }

    #[test]
    fn claims_count_per_key_and_drop_in_any_order() {
        let io = Io::new(Files::default());
        let minus = io.own_keys(OWN_MINUS);
        let plus = io.own_keys(OWN_PLUS);
        drop(minus);
        assert_eq!(
            io.keys_owned(),
            OWN_PLUS,
            "a claim dropped first leaves the later one"
        );
        let all = io.own_keys(OWN_ALL);
        drop(plus);
        assert_eq!(io.keys_owned(), OWN_ALL, "the other claim on + lives");
        let again = io.own_keys(OWN_MINUS);
        drop(all);
        assert_eq!(io.keys_owned(), OWN_MINUS, "a dead claim is not revived");
        drop(again);
        assert_eq!(io.keys_owned(), 0);
    }

    #[test]
    fn a_claim_ends_with_its_future_dropped_mid_wait_and_with_an_unwind() {
        let io = Io::new(Files::default());
        let mut wait = Box::pin(async {
            let _keys = io.own_keys(OWN_MINUS | OWN_EQUALS);
            io.vblank().await;
        });
        let mut cx = Context::from_waker(Waker::noop());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            io.keys_owned(),
            OWN_MINUS | OWN_EQUALS,
            "claimed while it waits"
        );
        drop(wait);
        assert_eq!(io.keys_owned(), 0, "the dropped future gave them back");
        let _outer = io.own_keys(OWN_PLUS);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _keys = io.own_keys(OWN_ALL);
            assert_eq!(io.keys_owned(), OWN_ALL);
            panic!("an unwind through a claim");
        }));
        assert!(unwound.is_err());
        assert_eq!(
            io.keys_owned(),
            OWN_PLUS,
            "the unwind gave back its own claim only"
        );
    }

    #[test]
    fn a_finished_game_is_not_polled_again() {
        let mut e = engine_with(|_state, _io| OnceFuture { done: false });
        e.tick(&[], 16);
        assert!(e.finished());
        e.tick(&[key('x')], 16);
        e.tick(&[], 16);
        assert_eq!(e.ticks(), 3);
        // Input after the end is still only queued.
        assert_eq!(e.io().pending_events(), 1);
    }
}

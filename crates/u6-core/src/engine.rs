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
}

/// The game's view of the platform: input queue, millisecond clock, display generation, two
/// screen buffers and the file layer. Nothing else in the core touches time, input or files.
pub struct Io {
    events: RefCell<VecDeque<InputEvent>>,
    now_ms: Cell<u64>,
    generation: Cell<u64>,
    back: RefCell<Framebuffer>,
    front: RefCell<Framebuffer>,
    front_version: Cell<u64>,
    files: Files,
    /// The layout of `game` in `files`, parsed at the first [`Io::game_image`] (`files` never
    /// changes, so it stays valid for the run).
    image: OnceCell<Option<crate::image::ImageLayout>>,
}

impl Io {
    fn new(files: Files) -> Self {
        Self {
            events: RefCell::new(VecDeque::new()),
            now_ms: Cell::new(0),
            generation: Cell::new(0),
            back: RefCell::new(Framebuffer::new()),
            front: RefCell::new(Framebuffer::new()),
            front_version: Cell::new(0),
            files,
            image: OnceCell::new(),
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
    pub fn next_key(&self) -> NextKey<'_> {
        NextKey { io: self }
    }

    /// Resolves once the tick clock reaches now + `ms` (deadline fixed at the call).
    pub fn sleep(&self, ms: u32) -> Sleep<'_> {
        Sleep {
            io: self,
            deadline: self.now_ms() + u64::from(ms),
        }
    }

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

    /// Marks the screen (front buffer) as shown: the engine publishes it after this poll. The
    /// Amiga displays its screen buffer continuously; the port publishes at the points where
    /// the original waits (key poll, VBlank), so a frame is never shown half drawn.
    pub fn present(&self) {
        self.front_version.set(self.front_version.get() + 1);
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
        match self.io.events.borrow_mut().pop_front() {
            Some(InputEvent::Key(key)) => Poll::Ready(key),
            None => Poll::Pending,
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
    published: Framebuffer,
    published_version: u64,
    stubs: Rc<StubLog>,
}

impl Engine {
    /// Boots the game from `files` with RNG seed word 0 (the image's own initial state).
    pub fn new(files: Files) -> Result<Engine, BootError> {
        Self::with_seed(files, 0)
    }

    /// Replaces: 0x14ef06 (system setup: memory, libraries, copper, VBlank server, input)
    ///
    /// Boots with an explicit RNG seed. The original seeds from the low word of `time()`
    /// (see [`crate::rng`]); platforms pass a clock value for variety or a constant for replays.
    pub fn with_seed(files: Files, seed: u16) -> Result<Engine, BootError> {
        let stubs = Rc::new(StubLog::default());
        let mut state = stub::scoped(&stubs, || State::new_game(&files))?;
        state.rng.seed(seed);
        Ok(Self::build(state, files, stubs, main_loop::run))
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
            published: Framebuffer::new(),
            published_version: 0,
            stubs,
        }
    }

    /// One host tick: queue `input`, advance the clock by `elapsed_ms`, poll the game once (also
    /// while it waits; never after it finished), bump the display generation, publish the front
    /// buffer if the game presented.
    pub fn tick(&mut self, input: &[InputEvent], elapsed_ms: u32) {
        self.io.events.borrow_mut().extend(input.iter().copied());
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
        if version != self.published_version {
            self.published.clone_from(&self.io.front.borrow());
            self.published_version = version;
        }
    }

    /// The last published frame: 320×200 indices plus the palette.
    pub fn frame(&self) -> &Framebuffer {
        &self.published
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

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
use std::sync::atomic::{AtomicU64, Ordering};
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

impl Key {
    /// Port (spec 19 O1c): one of the four direction keys, the ones a held key walks with at
    /// the game's pace ([`Engine::held_press`]).
    pub fn is_direction(self) -> bool {
        matches!(self, Key::Up | Key::Down | Key::Left | Key::Right)
    }
}

/// Port (spec 19 O1c): the shortest wall time between two steps of a held direction key: a
/// platform adds a press for it ([`Engine::held_press`]) only once it has been down this long
/// and the command prompt took its last press (the key's own or a paced one) this long ago
/// ([`paced_press_due`]). Above about 4.3× a turn ends within it (the castle's 640 ms is 80
/// wall ms at 8×), so a tap is one step at any speed and a held walk keeps an even cadence,
/// max(turn / speed, this).
pub const MIN_STEP_MS: u32 = 150;

/// Port (spec 19 O1c): whether a held direction key is due its next paced press: down
/// `down_ms` wall ms since its press, and `since_step_ms` since the command prompt last took a
/// press ([`Io::prompt_takes`]; None: never), both at least [`MIN_STEP_MS`]. The key's own
/// press is the platform's and comes at once; this paces only the presses added after it, from
/// the step the prompt last took, so the interval holds between steps however late a frame
/// delivered the one before.
pub fn paced_press_due(down_ms: f64, since_step_ms: Option<f64>) -> bool {
    let min = f64::from(MIN_STEP_MS);
    down_ms >= min && since_step_ms.is_none_or(|ms| ms >= min)
}

/// Port (spec 19 O1c): the command loop waits for a command key: A4-0x7a2c set (only around
/// the command read, 0x1041d8), no cross-hair or panel cursor (A4-0x7a60 clear) and key events
/// taken as commands (A4-0x7a42 set: no more prompt). There a platform paces a held direction
/// key ([`Engine::held_press`]) and the platform's own repeats of a direction key are taken
/// back ([`InputEvent::Repeat`]); every other wait reads them.
pub fn command_prompt(s: &State) -> bool {
    s.main.reading == 1 && s.ui.input.targeting == 0 && s.ui.input.accept
}

/// Port (spec 19 O1c): `e` is taken back at the command prompt: a platform's repeat of a
/// direction key, with `prompt` ([`command_prompt`]).
fn taken_back(prompt: bool, e: &InputEvent) -> bool {
    prompt && matches!(e, InputEvent::Repeat(k) if k.is_direction())
}

/// Port (spec 19 O1c): of the keys a platform has down, oldest first (each with what the
/// platform keeps beside it, such as its press time), the newest direction key: the one a
/// held walk goes on with. A repeat of an older key does not make it newer (a platform keeps
/// each held key at its first press), and once the newest is let go the one before it walks.
pub fn newest_direction<T>(held: impl DoubleEndedIterator<Item = (Key, T)>) -> Option<(Key, T)> {
    held.rev().find(|(k, _)| k.is_direction())
}

/// One input event, queued by the platform and consumed in order by the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputEvent {
    /// A key press: the first of a key held down, or any press of a platform that does not tell
    /// its auto-repeat apart (headless scripts).
    Key(Key),
    /// Port (spec 19 O1b): a press the platform's auto-repeat sent for a key still held, read as
    /// a press ([`Io::next_key`]) unless the key's [`InputEvent::Release`] took it back first.
    /// Port (spec 19 O1c): at the command prompt ([`command_prompt`]) a direction key's repeat
    /// is taken back, in the input of a tick and in the queue, so the paced press
    /// ([`InputEvent::Paced`]) is the one step source there; the cross-hair, the more prompt,
    /// the line editor and every other wait read it.
    Repeat(Key),
    /// Port (spec 19 O1c): a press of a held direction key the platform added at the command
    /// prompt ([`Engine::held_press`]), paced by [`MIN_STEP_MS`]. Read as a press; the key's
    /// release takes it back as it does a repeat, and the command prompt's take-back of the
    /// platform's repeats leaves it.
    Paced(Key),
    /// Port (spec 19 O1b): the key let go (platforms send it for each key a held physical key
    /// gave, whatever the modifiers are at the release). It is never queued and takes no slot of
    /// the ring ([`KEY_RING`]): where the engine queues the input ([`Engine::tick`]), while
    /// `game`'s key waits drain (A4-0x7a46 set, [`crate::ui::globals::InputState::drain`]: every
    /// wait but the line editor's) it takes the key's queued [`InputEvent::Repeat`]s out of the
    /// queue, so a held key stops at its release, and leaves the presses: a tap during a turn
    /// still runs once. In the line editor and in `u` (whose queue routine u 0x113ed0 skips a
    /// release's 0, so its ring at u's A4+0x2d56 never holds one) it takes nothing.
    ///
    /// Spec 19 O1 puts the cancel at the poll; it is done where the input is queued instead, so a
    /// release never waits in the queue: [`Io::pending_events`] stays the count of what the game
    /// has to take, which the prompt checks read (a host's at-prompt, the headless runner's busy
    /// test), and the pointer's in-order takes ([`Io::take_pointer`]) meet no release in front.
    /// The cost: repeats released while the flag is set are gone before a line editor opened
    /// later could read them (the original's editor would read them, its ring holding each).
    Release(Key),
    /// The pointer (mouse) at frame pixel (x, y), 0..[`u6_render::WIDTH`] ×
    /// 0..[`u6_render::HEIGHT`] (320 × 200 in Original; in Wide the map beside the UI too; the
    /// platform scales from its window; values outside are clamped), with `buttons` held: bit 0
    /// left, bit 1 right ([`POINTER_LEFT`], [`POINTER_RIGHT`]).
    Pointer { x: i16, y: i16, buttons: u8 },
    /// Port-only (spec 08 Design 4, slice K5a): one character the platform's input method
    /// committed (a Hangul syllable composed through an IME), queued in order with the keys. It
    /// passes by the keymap and never becomes a game code: only the line editor takes it
    /// ([`Io::next_typed`]); every other read of the keys drops it ([`Io::next_key`],
    /// [`Io::key_ready`]), so none is left for a later reader.
    Text(char),
}

/// What the line editor takes from the queue ([`Io::next_typed`]): a key press, or a character
/// of text ([`InputEvent::Text`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Typed {
    Key(Key),
    Text(char),
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
/// the frame, [`Pointer::apply`]) and the button word 0x161bd4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer {
    pub x: i16,
    pub y: i16,
    pub buttons: u8,
}

impl Pointer {
    /// Replaces: 0x151b08 (input.device handler, mouse class: the move is added to the position
    /// and clamped to 0..=319 / 0..=199; codes 0x68 / 0xe8 and 0x69 / 0xe9 set and clear
    /// button bits 0 and 1). Platforms report absolute positions, so the position is taken. The
    /// clamp is the frame's size, [`u6_render::WIDTH`] × [`u6_render::HEIGHT`]: the original's
    /// 320 × 200 in Original, the whole Wide frame in Wide (spec 13), where the map lies beside
    /// the UI.
    fn apply(x: i16, y: i16, buttons: u8) -> Pointer {
        Pointer {
            x: x.clamp(0, u6_render::WIDTH as i16 - 1),
            y: y.clamp(0, u6_render::HEIGHT as i16 - 1),
            buttons: buttons & (POINTER_LEFT | POINTER_RIGHT),
        }
    }
}

/// The number of the next [`Io`] made ([`Io::back_mark`]).
static NEXT_IO: AtomicU64 = AtomicU64::new(1);

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
    /// Port-only (spec 07 HD0): [`Io::present_sources`] was called since the last publish.
    sources_due: Cell<bool>,
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
    /// Port-only: this `Io`'s number, distinct for every `Io` made ([`NEXT_IO`]).
    serial: u64,
    /// Port-only: the mutable borrows of `back` so far ([`Io::draw_back`], [`Io::with_buffers`]).
    back_writes: Cell<u64>,
    /// Port-only: [`Io::set_window_cache`].
    window_cache: Cell<bool>,
    /// Port-only: [`Io::set_sight_memory`].
    sight_memory: Cell<bool>,
    /// Port (spec 19 O1c): the state of the game this `Io` serves, set once by the engine that
    /// made it, so a take reads whether the command prompt waits ([`command_prompt`]) as it is
    /// at the take.
    prompt_state: OnceCell<std::rc::Weak<RefCell<State>>>,
    /// Port (spec 19 O1c): [`Io::prompt_takes`].
    prompt_takes: Cell<u64>,
}

impl Io {
    pub(crate) fn new(files: Files) -> Self {
        Self {
            events: RefCell::new(VecDeque::new()),
            pointer: Cell::new(Pointer::default()),
            now_ms: Cell::new(0),
            generation: Cell::new(0),
            back: RefCell::new(Framebuffer::new()),
            front: RefCell::new(Framebuffer::new()),
            front_version: Cell::new(0),
            sources_due: Cell::new(false),
            published: RefCell::new(Framebuffer::new()),
            files,
            image: OnceCell::new(),
            owned_keys: Cell::new([0; OWN_KEYS]),
            serial: NEXT_IO.fetch_add(1, Ordering::Relaxed),
            back_writes: Cell::new(0),
            window_cache: Cell::new(true),
            sight_memory: Cell::new(false),
            prompt_state: OnceCell::new(),
            prompt_takes: Cell::new(0),
        }
    }

    /// Port (spec 19 O1c): the presses the command prompt has taken ([`command_prompt`] at the
    /// take: a key, a repeat it did not take back, a paced press), ever. A platform stamps the
    /// step's time when it moves ([`paced_press_due`]).
    pub fn prompt_takes(&self) -> u64 {
        self.prompt_takes.get()
    }

    /// Whether the command prompt waits now ([`command_prompt`]; false for an `Io` no engine
    /// serves, and while the state is borrowed mutably, which no take does).
    fn at_command_prompt(&self) -> bool {
        let Some(state) = self.prompt_state.get().and_then(std::rc::Weak::upgrade) else {
            return false;
        };
        let at = state.try_borrow().map(|s| command_prompt(&s));
        debug_assert!(
            at.is_ok(),
            "a key taken while the state is borrowed mutably"
        );
        at.unwrap_or(false)
    }

    /// One press taken: counted when the command prompt took it ([`Io::prompt_takes`]).
    fn took(&self, prompt: bool) {
        if prompt {
            self.prompt_takes.set(self.prompt_takes.get() + 1);
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
            InputEvent::Key(_)
            | InputEvent::Repeat(_)
            | InputEvent::Paced(_)
            | InputEvent::Release(_)
            | InputEvent::Text(_) => None,
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
    /// for the waits that read only keys; text before it is dropped (port, spec 08 slice K5a:
    /// a reader of keys takes no text).
    pub fn next_key(&self) -> NextKey<'_> {
        NextKey { io: self }
    }

    /// True when the next queued event is a key (a pointer event before it is taken first, by
    /// [`Io::take_pointer`]). Text at the front of the queue is dropped first (a reader of keys
    /// takes none), so it never holds back the events behind it; so, at the command prompt, is
    /// a repeat it takes back ([`InputEvent::Repeat`]).
    pub fn key_ready(&self) -> bool {
        let prompt = self.at_command_prompt();
        let mut events = self.events.borrow_mut();
        while let Some(e) = events.front()
            && (matches!(e, InputEvent::Text(_)) || taken_back(prompt, e))
        {
            events.pop_front();
        }
        matches!(
            events.front(),
            Some(InputEvent::Key(_) | InputEvent::Repeat(_) | InputEvent::Paced(_))
        )
    }

    /// Replaces: 0x134e7c (take a game code; here its drain with the flag set: the loop at
    /// 0x134e84..0x134e98 takes until the ring is empty and keeps the last code)
    ///
    /// Port (spec 19 O1b): the take of a key wait that drains (A4-0x7a46 set,
    /// [`crate::ui::keyread`]): every key press queued (presses and repeats) is taken and the
    /// newest is the one given. Text goes with them (a reader of keys takes none); pointer
    /// events keep their places (the original's pointer is no code of the ring). None when no
    /// key press is queued. At the command prompt the repeats it takes back go too, and are not
    /// given ([`InputEvent::Repeat`]).
    pub fn take_newest_key(&self) -> Option<Key> {
        let prompt = self.at_command_prompt();
        let mut newest = None;
        self.events.borrow_mut().retain(|e| match *e {
            _ if taken_back(prompt, e) => false,
            InputEvent::Key(k) | InputEvent::Repeat(k) | InputEvent::Paced(k) => {
                newest = Some(k);
                false
            }
            InputEvent::Text(_) | InputEvent::Release(_) => false,
            InputEvent::Pointer { .. } => true,
        });
        if newest.is_some() {
            self.took(prompt);
        }
        newest
    }

    /// Port-only (spec 08 Design 4, slice K5a): as [`Io::next_key`] for the line editor, which
    /// takes text too: resolves with the next queued key or character of text, in arrival order.
    pub fn next_typed(&self) -> NextTyped<'_> {
        NextTyped { io: self }
    }

    /// Port-only (spec 08 Design 4, slice K5a): as [`Io::key_ready`] for the line editor: true
    /// when the next queued event is a key or a character of text (nothing is dropped but, at
    /// the command prompt, the repeats it takes back, [`InputEvent::Repeat`]).
    pub fn typed_ready(&self) -> bool {
        let prompt = self.at_command_prompt();
        let mut events = self.events.borrow_mut();
        while events.front().is_some_and(|e| taken_back(prompt, e)) {
            events.pop_front();
        }
        matches!(
            events.front(),
            Some(
                InputEvent::Key(_)
                    | InputEvent::Repeat(_)
                    | InputEvent::Paced(_)
                    | InputEvent::Text(_)
            )
        )
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
        self.back_writes.set(self.back_writes.get() + 1);
        f(&mut self.back.borrow_mut())
    }

    /// Both buffers at once (back read-write, front read-write), for the copies between them.
    pub fn with_buffers<R>(&self, f: impl FnOnce(&mut Framebuffer, &mut Framebuffer) -> R) -> R {
        self.back_writes.set(self.back_writes.get() + 1);
        f(&mut self.back.borrow_mut(), &mut self.front.borrow_mut())
    }

    /// Reads the back buffer without counting a write ([`Io::back_mark`]), for tests that
    /// compare it.
    #[cfg(test)]
    pub(crate) fn peek_back<R>(&self, f: impl FnOnce(&Framebuffer) -> R) -> R {
        f(&self.back.borrow())
    }

    /// Port-only: this `Io`'s back buffer and the mutable borrows of it so far. A mark equal to
    /// an earlier one says the back buffer is the same one and nothing could draw on it since
    /// (every draw goes through [`Io::draw_back`] or [`Io::with_buffers`], which count; a read
    /// through them counts too).
    pub(crate) fn back_mark(&self) -> (u64, u64) {
        (self.serial, self.back_writes.get())
    }

    /// Port-only (spec 06 O2): with `on` (the default) the idle refresh draws only the cells of
    /// the map window whose draws read a changed value ([`crate::gfx::cache`]); off, it renders
    /// the whole window at every refresh, as the original does. The frames and the state are the
    /// same either way: the equivalence checks run both.
    pub fn set_window_cache(&self, on: bool) {
        self.window_cache.set(on);
    }

    /// [`Io::set_window_cache`].
    pub fn window_cache(&self) -> bool {
        self.window_cache.get()
    }

    /// Port-only (spec 18): with `on` the map window shows each cell in view that was seen and
    /// is out of sight now as it last showed it, its ground and still objects in dark grey
    /// ([`crate::gfx::cache::SightMemory`]); off (the default), such a cell is black, as on the
    /// original, and the frames are the original's. The page turns it on; the tests, the oracle
    /// comparisons and the receipts run with it off.
    pub fn set_sight_memory(&self, on: bool) {
        self.sight_memory.set(on);
    }

    /// [`Io::set_sight_memory`].
    pub fn sight_memory(&self) -> bool {
        self.sight_memory.get()
    }

    /// Port-only (spec 07 HD0): every pixel of the back buffer's, the screen's and the published
    /// frame's source planes classic, the colours untouched ([`Framebuffer::reset_src`]; the
    /// shown frame follows at its next sync from the published one). It counts as a write of the
    /// back buffer ([`Io::back_mark`]).
    pub fn reset_src_plane(&self) {
        self.draw_back(Framebuffer::reset_src);
        self.front.borrow_mut().reset_src();
        self.published.borrow_mut().reset_src();
    }

    /// Port-only (spec 07 HD0): gives the back buffer, the screen and the published frame a
    /// source plane ([`Framebuffer::enable_src`]), every pixel classic until drawn again. The
    /// colours are those without it. It counts as a write of the back buffer
    /// ([`Io::back_mark`]): the window cache's memo of what it holds no longer stands for the new
    /// plane.
    pub fn enable_src_plane(&self) {
        self.draw_back(Framebuffer::enable_src);
        self.front.borrow_mut().enable_src();
        self.published.borrow_mut().enable_src();
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

    /// Port-only (spec 07 HD0): the screen's sources changed where its colours did not (pixels
    /// put back in the colours they had, naming other images): the engine publishes the source
    /// plane alone after this poll, no new frame ([`Engine::src_version`]). A [`Io::present`] in
    /// the same poll publishes them anyway.
    pub fn present_sources(&self) {
        self.sources_due.set(true);
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
        let prompt = self.io.at_command_prompt();
        let mut events = self.io.events.borrow_mut();
        loop {
            match events.pop_front() {
                Some(e) if taken_back(prompt, &e) => {}
                Some(InputEvent::Key(key) | InputEvent::Repeat(key) | InputEvent::Paced(key)) => {
                    self.io.took(prompt);
                    return Poll::Ready(key);
                }
                Some(InputEvent::Pointer { x, y, buttons }) => {
                    self.io.pointer.set(Pointer::apply(x, y, buttons));
                }
                Some(InputEvent::Text(_) | InputEvent::Release(_)) => {}
                None => return Poll::Pending,
            }
        }
    }
}

/// Future of [`Io::next_typed`].
pub struct NextTyped<'a> {
    io: &'a Io,
}

impl Future for NextTyped<'_> {
    type Output = Typed;
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Typed> {
        let prompt = self.io.at_command_prompt();
        let mut events = self.io.events.borrow_mut();
        loop {
            match events.pop_front() {
                Some(e) if taken_back(prompt, &e) => {}
                Some(InputEvent::Key(key) | InputEvent::Repeat(key) | InputEvent::Paced(key)) => {
                    self.io.took(prompt);
                    return Poll::Ready(Typed::Key(key));
                }
                Some(InputEvent::Text(c)) => return Poll::Ready(Typed::Text(c)),
                Some(InputEvent::Pointer { x, y, buttons }) => {
                    self.io.pointer.set(Pointer::apply(x, y, buttons));
                }
                Some(InputEvent::Release(_)) => {}
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
/// full ring differs from an empty one; it tests nothing else); a key that finds it full is
/// dropped. The port counts presses and repeats ([`InputEvent::Key`], [`InputEvent::Repeat`]).
/// The original's handler 0x151b08 pushes every key event, a release too: 0x151d00 maps a
/// release (codes from 0x80) to 0, except for an Alt release that ends a number typed on the
/// keypad, which gives that number ([`crate::ui::input::map_raw`]), and the push at
/// 0x151c32..0x151c3c does not test it, so there a released key takes two slots and here one
/// ([`InputEvent::Release`] takes none). Only the line editor reads the ring in order
/// (A4-0x7a46 clear: 0x107cd6 clears it, 0x107d5a sets it back); every other key wait empties
/// it at its take and keeps the last code (0x105d06..0x105d0e hands the flag to 0x134e7c, whose
/// loop at 0x134e84..0x134e98 takes until the ring is empty), so there the room only bounds
/// what one take drops.
pub(crate) const KEY_RING: usize = 49;

/// Port-only (spec 08 Design 4, slice K5a): the characters of text ([`InputEvent::Text`]) the
/// queue holds at most, counted apart from the keys of [`KEY_RING`] (text takes no slot of the
/// handler's ring, which has none); one that finds it full is dropped. As many as the ring's
/// keys: more than a line of the conversation takes ([`crate::converse::INPUT_MAX`] bytes).
const TEXT_RING: usize = KEY_RING;

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
    /// The screen's version ([`Io::present`]) the published frame was synced from last.
    published_front: u64,
    /// Bumped each time the published frame is synced again from the screen: its colours and
    /// sources, or its sources alone ([`Engine::published_version`]).
    published_version: u64,
    /// Port-only (spec 17 Presentation): the published frame with the context menu over it
    /// ([`Engine::screen_frame`]): synced from the published frame, the one frame `shown` syncs
    /// from.
    layer: Framebuffer,
    /// Bumped each time `layer` is synced again for a change ([`Engine::screen_version`]).
    layer_version: u64,
    /// Port-only (spec 17): the context menu, its run and its share of the input.
    popup: crate::ui::popup::Popup,
    /// The [`crate::ui::popup::Popup::version`] drawn into `layer` last.
    drawn_popup: u64,
    /// Port-only (spec 17, unit M4 seam): the stub's record of what "Go here" asked.
    go_here: crate::ui::popup::Travel,
    /// Port-only (spec 17): the button-down edges of the pointer events the menu let through to
    /// the game ([`Engine::game_presses`]), and the buttons of the last of them.
    game_presses: u64,
    passed_buttons: u8,
    /// Port-only (spec 17): the key events (presses, repeats, releases) the menu let through to
    /// the game ([`Engine::game_keys`]).
    game_keys: u64,
    /// The screen frame with the pointer sprite over it (the frame the platform shows).
    shown: Framebuffer,
    /// Bumped each time `shown` is composed again ([`Engine::frame_version`]).
    shown_version: u64,
    /// Bumped each time only `shown`'s source plane changed ([`Engine::src_version`]).
    src_version: u64,
    /// The ticks whose publish took the source plane alone ([`Engine::src_publishes`]).
    #[cfg(test)]
    src_publishes: u64,
    /// The ticks that composed `shown` again from such a publish ([`Engine::src_shows`]).
    #[cfg(test)]
    src_shows: u64,
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
        let _ = io.prompt_state.set(Rc::downgrade(&state));
        let task: Task = Box::pin(game(Rc::clone(&state), Rc::clone(&io)));
        Engine {
            state,
            io,
            task,
            finished: false,
            ticks: 0,
            published_front: 0,
            published_version: 0,
            layer: Framebuffer::new(),
            layer_version: 0,
            popup: crate::ui::popup::Popup::default(),
            drawn_popup: 0,
            go_here: crate::ui::popup::Travel::default(),
            game_presses: 0,
            passed_buttons: 0,
            game_keys: 0,
            shown: Framebuffer::new(),
            shown_version: 0,
            src_version: 0,
            #[cfg(test)]
            src_publishes: 0,
            #[cfg(test)]
            src_shows: 0,
            sprite: None,
            stubs,
        }
    }

    /// One host tick: queue `input` (keys past the handler's ring, [`KEY_RING`], and text past
    /// [`TEXT_RING`] are dropped; a release is applied, [`InputEvent::Release`]; at the command
    /// prompt the repeats it takes back are dropped, from the queue too, [`InputEvent::Repeat`]),
    /// advance the clock by `elapsed_ms`, poll the game once (also
    /// while it waits; never after it finished), bump the display generation, publish the front
    /// buffer if the game presented, then the vertical-blank work of the display (the pointer
    /// sprite, at the handler's position, over the published frame,
    /// [`crate::ui::pointer::vblank`]).
    ///
    /// Port-only (spec 17): with the context menu on, `input` goes through it first
    /// ([`crate::ui::popup::Popup::input`]): what it takes never reaches the queue, and a choice's
    /// presses take their place; the menu is drawn over the published frame ([`Engine::screen_frame`]).
    pub fn tick(&mut self, input: &[InputEvent], elapsed_ms: u32) {
        let through;
        let input = if self.popup.on() {
            let quiet = self.io.pending_events() == 0;
            through = self
                .popup
                .input(&self.state.borrow(), &self.io, input, quiet);
            for e in &through {
                match *e {
                    InputEvent::Pointer { buttons, .. } => {
                        let b = buttons & 3;
                        self.game_presses += u64::from(b & !self.passed_buttons != 0);
                        self.passed_buttons = b;
                    }
                    InputEvent::Key(_) | InputEvent::Repeat(_) | InputEvent::Release(_) => {
                        self.game_keys += 1;
                    }
                    _ => {}
                }
            }
            // Unit M4's seam (spec 21): "Go here" chosen in this input starts travel to its cell
            // after the menu's filter.
            if let Some(cell) = self.popup.take_go_here() {
                crate::ui::popup::travel_to(&mut self.go_here, cell);
            }
            through.as_slice()
        } else {
            input
        };
        {
            let mut events = self.io.events.borrow_mut();
            let count = |text: bool| {
                events
                    .iter()
                    .filter(|e| matches!(e, InputEvent::Text(_)) == text)
                    .filter(|e| !matches!(e, InputEvent::Pointer { .. }))
                    .count()
            };
            let (mut keys, mut texts) = (count(false), count(true));
            let (cancel, prompt) = {
                let s = self.state.borrow();
                let cancel = s.main.program == main_loop::Program::Game && s.ui.input.drain;
                (cancel, !self.finished && command_prompt(&s))
            };
            if prompt {
                let before = events.len();
                events.retain(|q| !taken_back(true, q));
                keys -= before - events.len();
            }
            for &e in input {
                let (held, room) = match e {
                    _ if taken_back(prompt, &e) => continue,
                    InputEvent::Key(_) | InputEvent::Repeat(_) | InputEvent::Paced(_) => {
                        (&mut keys, KEY_RING)
                    }
                    InputEvent::Text(_) => (&mut texts, TEXT_RING),
                    InputEvent::Pointer { .. } => {
                        events.push_back(e);
                        continue;
                    }
                    InputEvent::Release(k) => {
                        if cancel {
                            let before = events.len();
                            events.retain(|q| {
                                *q != InputEvent::Repeat(k) && *q != InputEvent::Paced(k)
                            });
                            keys -= before - events.len();
                        }
                        continue;
                    }
                };
                if *held >= room {
                    continue;
                }
                *held += 1;
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
        self.popup.after_poll(&self.state.borrow());
        self.ticks += 1;
        self.io.generation.set(self.io.generation() + 1);
        let version = self.io.front_version.get();
        let presented = version != self.published_front;
        // The published frame is the one buffer synced from the screen, and the shown frame the
        // one synced from it: with a source plane only its changed chunks are copied. Sources
        // presented alone ([`Io::present_sources`]) are published with the plane alone, where
        // the colours already are the screen's, and the shown frame composed again in the same
        // colours; the colours and the frames are as without them.
        let sources = self.io.sources_due.replace(false);
        let mut src_only = false;
        if presented {
            self.io
                .published
                .borrow_mut()
                .sync_from(&mut self.io.front.borrow_mut());
            self.published_front = version;
            self.published_version += 1;
        } else if sources {
            let mut front = self.io.front.borrow_mut();
            src_only = self.io.published.borrow_mut().sync_src_from(&mut front) > 0;
            self.published_version += u64::from(src_only);
            #[cfg(test)]
            {
                self.src_publishes += u64::from(src_only);
            }
        }
        // Port-only (spec 17 Presentation): the screen frame is the published one with the menu
        // over it, synced again (and the menu drawn again over it) whenever either changed.
        let drawn = self.popup.version();
        let menu_moved = drawn != self.drawn_popup;
        let layered = presented || menu_moved;
        if layered || src_only || self.ticks == 1 {
            self.layer.sync_from(&mut self.io.published.borrow_mut());
            if let Some(open) = self.popup.open() {
                open.draw(&mut self.layer, &self.state.borrow());
            }
            self.drawn_popup = drawn;
            self.layer_version += u64::from(layered || src_only);
        }
        let at = self.io.handler_pointer();
        let mut sprite = crate::ui::pointer::vblank(&mut self.state.borrow_mut(), at);
        if let Some(p) = self.popup.pointer() {
            // The menu's pointer: the game's sample stays where the game took it. In the arrow
            // over the open menu, else (a press it took, held) in the game's own shape.
            let ptr = &self.state.borrow().ui.pointer;
            let shape = if self.popup.open().is_some() {
                0
            } else {
                ptr.shape
            };
            sprite = crate::ui::pointer::sprite_as(ptr, shape, p);
        }
        if layered || sprite != self.sprite || self.ticks == 1 {
            self.shown.sync_from(&mut self.layer);
            if let Some(sp) = &sprite {
                sp.draw(&mut self.shown);
            }
            self.sprite = sprite;
            self.shown_version += 1;
        } else if src_only {
            // As the composition above, but for the frame version: the published frame's
            // colours are those `shown` was composed from and the sprite has not moved, so the
            // colours come out the same; the sprite's pixels are classic again. The source version
            // moves only if the sources shown changed (not for a change under the sprite alone).
            let before = self.shown.src().map(<[crate::Src]>::to_vec);
            #[cfg(debug_assertions)]
            let colours = self.shown.pixels().to_vec();
            self.shown.sync_from(&mut self.layer);
            if let Some(sp) = &self.sprite {
                sp.draw(&mut self.shown);
            }
            #[cfg(debug_assertions)]
            debug_assert!(
                self.shown.pixels() == colours,
                "a source-only publish keeps the colours"
            );
            if self.shown.src() != before.as_deref() {
                self.src_version += 1;
            }
            #[cfg(test)]
            {
                self.src_shows += 1;
            }
        }
    }

    /// The pointer sprite drawn over the shown frame last ([`crate::ui::pointer::vblank`]'s, at
    /// the handler's position; spec 17: the arrow at the context menu's pointer while the menu
    /// takes the pointer), if any: its top left in frame pixels and its rows of values
    /// 0..=3 (0 clear). Port-only use (spec 11 Design 7): a platform that shows the HD frame,
    /// composed without it ([`Engine::published_frame`]), draws it over that frame itself.
    pub fn sprite(&self) -> Option<&crate::ui::pointer::SpriteFrame> {
        self.sprite.as_ref()
    }

    /// Port-only (spec 11 Design 7): the colour the shown frame draws sprite value `v` in, its
    /// palette's entry 16 + v ([`crate::ui::pointer::SpriteFrame::colour_index`]); `None` for 0
    /// (clear).
    pub fn sprite_colour(&self, v: u8) -> Option<crate::Rgb> {
        crate::ui::pointer::SpriteFrame::colour_index(v).map(|i| self.shown.palette[usize::from(i)])
    }

    /// The ticks so far whose publish took the source plane alone, the screen's sources into
    /// the published frame where the colours already were its own ([`Io::present_sources`]; tests).
    #[cfg(test)]
    pub(crate) fn src_publishes(&self) -> u64 {
        self.src_publishes
    }

    /// The ticks so far that composed the shown frame again from a publish of the plane alone,
    /// the pointer sprite standing, in the colours it had ([`Engine::src_publishes`]; tests).
    #[cfg(test)]
    pub(crate) fn src_shows(&self) -> u64 {
        self.src_shows
    }

    /// The last published frame (320×200 indices plus the palette), the context menu over it
    /// while one is open ([`Engine::screen_frame`]), with the pointer sprite.
    pub fn frame(&self) -> &Framebuffer {
        &self.shown
    }

    /// Port-only (spec 11 Design 7): the last published frame without the pointer sprite, the
    /// screen as the game drew it (what [`Engine::frame`] is composed from; the original's
    /// pointer is a hardware sprite above the screen, `ui/pointer.rs`). The HD frame is composed
    /// from it. Borrowed between ticks only (a tick publishes into it).
    pub fn published_frame(&self) -> Ref<'_, Framebuffer> {
        self.io.published.borrow()
    }

    /// Port-only (spec 11 Design 7): changes whenever [`Engine::published_frame`] was synced
    /// again: the game presented (its colours and sources), or its sources alone were published
    /// ([`Io::present_sources`]); 0 before the first publish. A move of the pointer sprite alone
    /// leaves it, so a consumer of the published frame composes nothing for one.
    pub fn published_version(&self) -> u64 {
        self.published_version
    }

    /// Port-only (spec 17 Presentation): the published frame with the context menu drawn over
    /// it, without the pointer sprite: what [`Engine::frame`] is composed from, and the HD frame.
    /// The published frame itself while no menu is open.
    pub fn screen_frame(&self) -> &Framebuffer {
        &self.layer
    }

    /// Port-only (spec 17): changes whenever [`Engine::screen_frame`] was synced again for a
    /// change: a publish ([`Engine::published_version`]) or a change of the drawn menu; 0 before.
    pub fn screen_version(&self) -> u64 {
        self.layer_version
    }

    /// Port-only (spec 17): the context menu on or off (off by default: a right press is the
    /// original's; off closes an open menu and drops a run).
    pub fn set_context_menu(&mut self, on: bool) {
        self.popup.set_on(on);
        self.redraw_menu();
    }

    /// Port-only (spec 17 Presentation): the frame rects (x0, y0, x1, y1 inclusive) the platform
    /// shows something else over: a context menu opened later keeps off them where it can.
    pub fn set_context_menu_hidden(&mut self, rects: Vec<crate::ui::popup::Rect>) {
        self.popup.set_hidden(rects, &self.state.borrow());
        self.redraw_menu();
    }

    /// Port-only (spec 17 Presentation): closes an open context menu as a press outside it
    /// would, for a press of the platform's that reaches no frame pixel (off the canvas, in the
    /// HD layout's gaps or the letterbox); true when one was open.
    pub fn dismiss_context_menu(&mut self) -> bool {
        let open = self.popup.dismiss();
        self.redraw_menu();
        open
    }

    /// The screen frame and the shown frame again when the drawn menu changed outside a tick
    /// (laid out again, dismissed), as a tick composes them.
    fn redraw_menu(&mut self) {
        let drawn = self.popup.version();
        if drawn == self.drawn_popup {
            return;
        }
        self.layer.sync_from(&mut self.io.published.borrow_mut());
        if let Some(open) = self.popup.open() {
            open.draw(&mut self.layer, &self.state.borrow());
        }
        self.drawn_popup = drawn;
        self.layer_version += 1;
        self.shown.sync_from(&mut self.layer);
        if let Some(sp) = &self.sprite {
            sp.draw(&mut self.shown);
        }
        self.shown_version += 1;
    }

    /// Port-only (spec 17): the presses the context menu let through to the game (button-down
    /// edges of the pointer events it gave the queue), counted while it is on.
    pub fn game_presses(&self) -> u64 {
        self.game_presses
    }

    /// Port-only (spec 17): the key events (presses, repeats and releases) the context menu let
    /// through to the game, counted while it is on; the keys it takes itself are left out.
    pub fn game_keys(&self) -> u64 {
        self.game_keys
    }

    /// Port-only (spec 17): a chosen command's target press waits for its target read.
    pub fn context_menu_waits(&self) -> bool {
        self.popup.run().is_some()
    }

    /// Port-only (spec 11 Design 7, K3): where the pointer is shown: the menu's while it takes
    /// the pointer's events, else the input handler's.
    pub fn pointer_at(&self) -> Pointer {
        self.popup
            .pointer()
            .unwrap_or_else(|| self.io.handler_pointer())
    }

    /// Port-only (spec 17): the open context menu as drawn (no text), if one is open.
    pub fn context_menu(&self) -> Option<crate::ui::popup::View> {
        self.popup.open().map(crate::ui::popup::Open::view)
    }

    /// The context menu itself (tests).
    #[cfg(test)]
    pub(crate) fn popup_mut(&mut self) -> &mut crate::ui::popup::Popup {
        &mut self.popup
    }

    /// Port-only (spec 17, unit M4 seam): the cells "Go here" asked travel to (the stub's record
    /// until unit M4's `travel_to` lands, spec 21).
    pub fn go_here_asked(&self) -> &crate::ui::popup::Travel {
        &self.go_here
    }

    /// Port-only (spec 07 HD0): every buffer gets a source plane ([`Io::enable_src_plane`], and
    /// the frame with the pointer sprite, whose pixels are classic). Off unless called; the
    /// frames' colours are the same either way.
    pub fn enable_src_plane(&mut self) {
        self.io.enable_src_plane();
        self.layer.enable_src();
        self.shown.enable_src();
    }

    /// Changes whenever [`Engine::frame`] was composed again (the game presented, the pointer
    /// sprite changed, the first tick); 0 before the first tick. A platform converts the frame
    /// only when this moved since its last conversion (spec 06 O1): a tick that composed nothing
    /// left the frame as it was.
    pub fn frame_version(&self) -> u64 {
        self.shown_version
    }

    /// Port-only (spec 07 HD0): changes whenever only the source plane of [`Engine::frame`]
    /// changed, its colours not ([`Io::present_sources`]; not for a change under the pointer
    /// sprite alone, whose pixels are classic); 0 until then. A consumer of the plane recomposes
    /// when this or [`Engine::frame_version`] moved.
    pub fn src_version(&self) -> u64 {
        self.src_version
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

    /// Port-only (spec 09 Design 1): voices on or off: the speech tap keeps what NPCs say
    /// ([`crate::speech`]) only while on. Off by default, and then nothing is kept.
    pub fn set_speech(&mut self, on: bool) {
        crate::speech::set_enabled(&mut self.state.borrow_mut(), on);
    }

    /// Port-only (spec 08): installs the locale pack `pack` on the game, checked against its
    /// files first ([`crate::locale::install`]), with the Hangul table `hangul` (from the
    /// platform). A mismatch changes nothing.
    pub fn install_locale(
        &mut self,
        pack: Rc<crate::locale::Pack>,
        hangul: Option<Rc<u6_formats::font::HangulFont>>,
    ) -> Result<(), crate::locale::Mismatch> {
        crate::locale::install(&mut self.state.borrow_mut(), self.io.files(), pack, hangul)
    }

    /// Port-only (spec 09 Design 1): the utterances NPCs said since the previous call (a channel
    /// apart from the log; the text is the user's game text and is never logged).
    pub fn take_speech(&mut self) -> Vec<crate::speech::Utterance> {
        crate::speech::take(&mut self.state.borrow_mut())
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

    /// Port (spec 19 O1c): the press a platform adds to `input` before a tick for `held`, the
    /// direction key it has down ([`newest_direction`]; a walking one: first pressed without
    /// Ctrl, Alt or Meta, and due by [`paced_press_due`]): its paced press
    /// ([`InputEvent::Paced`]) when the command loop waits for a command key
    /// ([`command_prompt`]) and no key waits, neither queued nor in `input` (pointer events may,
    /// and the platform's repeats of direction keys, which the prompt takes back). So a held
    /// direction key is pressed again at the first command prompt it is due at and walks one
    /// step a turn or one per [`MIN_STEP_MS`], whichever is longer, the one step source there;
    /// every other wait reads the platform's own repeat. None otherwise, and for any other key.
    pub fn held_press(&self, input: &[InputEvent], held: Option<Key>) -> Option<InputEvent> {
        let key = |e: &InputEvent| !matches!(e, InputEvent::Pointer { .. }) && !taken_back(true, e);
        let held = held.filter(|k| k.is_direction())?;
        let prompt = !self.finished && command_prompt(&self.state.borrow());
        let waiting = input.iter().any(key) || self.io.events.borrow().iter().any(key);
        (prompt && !waiting).then_some(InputEvent::Paced(held))
    }

    /// Port (spec 19 O1c): [`Io::prompt_takes`].
    pub fn prompt_takes(&self) -> u64 {
        self.io.prompt_takes()
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
        // counts presses and repeats (a release takes a slot there too, none here). Pointer
        // events are no keys.
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

    /// Spec 19 O1c (review S1 and NIT, round 4 item 1): every branch of
    /// [`Engine::held_press`]. At the command prompt (A4-0x7a2c set, no cross-hair, A4-0x7a42
    /// set) with nothing waiting a held direction key gives its paced press, pointer events or
    /// not, and the platform's repeats of direction keys (taken back there) or not; a key
    /// queued, a key, a letter's repeat or a release in this step's input, a game finished,
    /// another key, no key, another wait (A4-0x7a2c clear), a cross-hair (A4-0x7a60 set) or a
    /// more prompt (A4-0x7a42 clear) give none.
    #[test]
    fn a_held_press_comes_only_at_a_quiet_command_prompt() {
        let engine = |reading: u16, targeting: u8, accept: bool, ends: bool| {
            let mut s = State::empty();
            s.main.reading = reading;
            s.ui.input.targeting = targeting;
            s.ui.input.accept = accept;
            Engine::with_game(s, Files::default(), move |_state, io| async move {
                if !ends {
                    io.sleep(u32::MAX).await;
                }
            })
        };
        let left = Some(Key::Left);
        let pointer = InputEvent::Pointer {
            x: 1,
            y: 2,
            buttons: 0,
        };
        let paced = Some(InputEvent::Paced(Key::Left));
        let os = InputEvent::Repeat(Key::Down);
        let e = engine(1, 0, true, false);
        assert_eq!(e.held_press(&[], left), paced, "quiet prompt");
        assert_eq!(e.held_press(&[pointer], left), paced, "pointer events");
        assert_eq!(e.held_press(&[os], left), paced, "a direction's repeat");
        assert_eq!(e.held_press(&[key('a')], left), None, "a key in the input");
        let letter = InputEvent::Repeat(Key::Char('a'));
        assert_eq!(e.held_press(&[letter], left), None, "a letter's repeat");
        let up = InputEvent::Release(Key::Down);
        assert_eq!(e.held_press(&[up], left), None, "a release in the input");
        assert_eq!(
            e.held_press(&[], Some(Key::Char('x'))),
            None,
            "not a direction"
        );
        assert_eq!(e.held_press(&[], None), None, "none held");
        let mut e = engine(1, 0, true, false);
        e.tick(&[pointer], 16);
        assert_eq!(e.held_press(&[], left), paced, "a pointer event queued");
        let mut e = engine(1, 0, true, false);
        e.tick(&[key('a')], 16);
        assert_eq!(e.held_press(&[], left), None, "a key queued");
        assert_eq!(
            engine(0, 0, true, false).held_press(&[], left),
            None,
            "another wait"
        );
        assert_eq!(
            engine(1, 1, true, false).held_press(&[], left),
            None,
            "a cross-hair"
        );
        assert_eq!(
            engine(1, 0, false, false).held_press(&[], left),
            None,
            "a more prompt"
        );
        let mut e = engine(1, 0, true, true);
        e.tick(&[], 16);
        assert!(e.finished());
        assert_eq!(e.held_press(&[], left), None, "finished");
    }

    /// Spec 19 O1c (round 4 item 1): at the command prompt ([`command_prompt`]) the platform's
    /// repeats of direction keys are taken back, in a tick's input and in the queue, also those
    /// queued while a turn ran and met by a prompt that comes up within a tick (the take reads
    /// the prompt as it is then); a press, a paced press and a letter's repeat are read, and
    /// counted ([`Io::prompt_takes`]). The cross-hair, the more prompt and every other wait
    /// (A4-0x7a2c clear) read the repeats. A game that waits 100 ms with A4-0x7a2c clear, then
    /// sets it with `targeting` and `accept` and reads one key, over and over.
    #[test]
    fn the_command_prompt_takes_back_the_platforms_repeats_of_direction_keys() {
        let run = |targeting: u8, accept: bool, input: &[(u32, InputEvent)]| {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let out = Rc::clone(&seen);
            let mut s = State::empty();
            s.ui.input.targeting = targeting;
            s.ui.input.accept = accept;
            let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
                loop {
                    io.sleep(100).await;
                    st.borrow_mut().main.reading = 1;
                    let k = io.next_key().await;
                    st.borrow_mut().main.reading = 0;
                    out.borrow_mut().push(k);
                }
            });
            for t in 0..60 {
                let now: Vec<InputEvent> = input
                    .iter()
                    .filter(|(at, _)| *at == t)
                    .map(|&(_, e)| e)
                    .collect();
                e.tick(&now, 20);
            }
            let read = seen.borrow().clone();
            (read, e.prompt_takes())
        };
        let (os, paced) = (InputEvent::Repeat(Key::Left), InputEvent::Paced(Key::Left));
        let letter = InputEvent::Repeat(Key::Char('a'));
        // Repeats queued during the first wait (ticks 0..4), the prompt up from the tick that
        // ends it; then a repeat at the prompt, a paced press, a press and a letter's repeat.
        let input = [
            (0, os),
            (2, os),
            (4, os),
            (8, os),
            (9, paced),
            (16, key('x')),
            (24, letter),
            (26, os),
        ];
        let (read, takes) = run(0, true, &input);
        assert_eq!(
            read,
            [Key::Left, Key::Char('x'), Key::Char('a')],
            "the paced press, the press and the letter's repeat"
        );
        assert_eq!(takes, 3, "counted");
        for (targeting, accept, what) in [(1, true, "a cross-hair"), (0, false, "a more prompt")] {
            let (read, takes) = run(targeting, accept, &input);
            assert_eq!(read.len(), input.len(), "{what} reads them all: {read:?}");
            assert_eq!(takes, 0, "{what}: not the command prompt");
        }
        let mut s = State::empty();
        s.ui.input.accept = true;
        let mut e = Engine::with_game(s, Files::default(), |_st, io| async move {
            for _ in 0..3 {
                io.next_key().await;
            }
        });
        e.tick(&[os, os, os], 20);
        assert!(e.finished(), "another wait (A4-0x7a2c clear) reads them");
    }

    /// Spec 19 O1c: a paced press waiting in the queue is taken back by its key's release, as a
    /// repeat is, while the key waits drain.
    #[test]
    fn a_release_takes_back_a_paced_press() {
        let mut s = State::empty();
        s.main.program = main_loop::Program::Game;
        s.ui.input.drain = true;
        let mut e = Engine::with_game(s, Files::default(), |_state, io| async move {
            io.sleep(u32::MAX).await;
        });
        e.tick(&[InputEvent::Paced(Key::Left)], 16);
        assert_eq!(e.io().pending_events(), 1);
        e.tick(&[InputEvent::Release(Key::Left)], 16);
        assert_eq!(e.io().pending_events(), 0, "taken back");
    }

    /// Spec 19 O1c (D1', round 4 item 3): [`paced_press_due`] at its edges: a key down less
    /// than [`MIN_STEP_MS`] is not due whatever the last step; down that long it is due with no
    /// step yet or the last one that old, and not with one sooner.
    #[test]
    fn a_held_walk_is_due_once_down_and_stepped_the_minimum_interval_ago() {
        let min = f64::from(MIN_STEP_MS);
        assert!(!paced_press_due(min - 1.0, None), "a tap");
        assert!(!paced_press_due(min - 1.0, Some(1000.0)), "down too short");
        assert!(paced_press_due(min, None), "no step yet");
        assert!(
            paced_press_due(1000.0, Some(min)),
            "the last step old enough"
        );
        assert!(
            !paced_press_due(1000.0, Some(min - 1.0)),
            "the last step too recent"
        );
        assert!(!paced_press_due(1000.0, Some(0.0)), "a step this frame");
    }

    #[test]
    fn a_release_takes_back_its_keys_queued_repeats_while_game_drains() {
        // Spec 19 O1b: in `game` with the drain flag A4-0x7a46 set, a release takes the key's
        // queued repeats out, their room with them, and leaves its press and the other keys;
        // with the flag clear (the line editor) and in `u` it takes nothing. It is never queued.
        use main_loop::Program;
        let engine = |program, drain| {
            let mut s = State::empty();
            s.main.program = program;
            s.ui.input.drain = drain;
            Engine::with_game(s, Files::default(), |_state, io| async move {
                io.sleep(u32::MAX).await;
            })
        };
        let (a, b) = (Key::Char('a'), Key::Char('b'));
        let (press, repeat) = (InputEvent::Key, InputEvent::Repeat);
        let input = [
            press(a),
            repeat(a),
            repeat(b),
            repeat(a),
            InputEvent::Release(a),
            repeat(a),
        ];
        let all = vec![press(a), repeat(a), repeat(b), repeat(a), repeat(a)];
        for (program, drain, want) in [
            (Program::Game, true, vec![press(a), repeat(b), repeat(a)]),
            (Program::Game, false, all.clone()),
            (Program::U, true, all),
        ] {
            let mut e = engine(program, drain);
            e.tick(&input, 16);
            let queued: Vec<InputEvent> = e.io.events.borrow().iter().copied().collect();
            assert_eq!(queued, want, "{program:?}, drain {drain}");
        }
        // The press taken, its repeats queued, the release, then a new press of the same key
        // and its release before the next take: the new press is all that is left.
        let mut e = engine(Program::Game, true);
        let up = InputEvent::Release;
        e.tick(&[repeat(a), repeat(a), up(a), press(a), up(a)], 16);
        let queued: Vec<InputEvent> = e.io.events.borrow().iter().copied().collect();
        assert_eq!(queued, [press(a)], "the new press survives its release");
        // The room comes back within the tick: a full ring of repeats, their release, a full
        // ring of presses, all in one input.
        let mut e = engine(Program::Game, true);
        let mut input = vec![repeat(a); KEY_RING];
        input.push(up(a));
        input.extend([press(b); KEY_RING]);
        e.tick(&input, 16);
        let queued: Vec<InputEvent> = e.io.events.borrow().iter().copied().collect();
        assert_eq!(
            queued,
            [press(b); KEY_RING],
            "the released repeats' room taken again"
        );
    }

    #[test]
    fn text_has_its_own_room_and_takes_none_of_the_keys() {
        // Port (spec 08 slice K5a): text is no key of the ring; it is bounded apart, in order
        // with the keys.
        let mut e = idle_engine();
        let mut input: Vec<InputEvent> = (0..60).map(|_| InputEvent::Text('\u{ac00}')).collect();
        input.extend((0..60).map(|_| key('a')));
        e.tick(&input, 16);
        let queued: Vec<InputEvent> = e.io.events.borrow().iter().copied().collect();
        let texts = queued
            .iter()
            .filter(|ev| matches!(ev, InputEvent::Text(_)))
            .count();
        let keys = queued.len() - texts;
        println!("60 characters and 60 keys queued: {texts} characters and {keys} keys wait");
        assert_eq!((texts, keys), (TEXT_RING, KEY_RING));
        assert!(matches!(queued[0], InputEvent::Text(_)), "in arrival order");
        assert!(matches!(queued[TEXT_RING], InputEvent::Key(_)));
    }

    #[test]
    fn a_reader_of_keys_drops_the_text_before_a_key_and_the_line_editor_takes_it() {
        // Port (spec 08 slice K5a): a key wait takes the key behind text and leaves no text
        // queued; `key_ready` drops text in front so the pointer and keys behind it come; the
        // line editor's read takes both, in order.
        let log = Rc::new(RefCell::new(Vec::new()));
        let out = Rc::clone(&log);
        let mut e = engine_with(move |_state, io| async move {
            let k = io.next_key().await;
            out.borrow_mut().push(format!("{k:?}"));
            for _ in 0..3 {
                let t = io.next_typed().await;
                out.borrow_mut().push(format!("{t:?}"));
            }
        });
        let text = InputEvent::Text('\u{ac00}');
        e.tick(&[text, text, key('a')], 16);
        assert_eq!(*log.borrow(), ["Char('a')"]);
        assert_eq!(
            e.io().pending_events(),
            0,
            "no text left for a later reader"
        );
        e.tick(&[text, key('b'), text], 16);
        assert_eq!(
            log.borrow()[1..],
            ["Text('\u{ac00}')", "Key(Char('b'))", "Text('\u{ac00}')"]
        );
        let io = Io::new(Files::default());
        io.events.borrow_mut().extend([text, text]);
        assert!(!io.key_ready(), "no key");
        assert_eq!(io.pending_events(), 0, "the text dropped");
        io.events.borrow_mut().extend([text, key('c')]);
        assert!(io.typed_ready(), "the line editor's read sees the text");
        assert_eq!(io.pending_events(), 2, "and drops nothing");
        assert!(io.key_ready(), "a key behind the text");
        assert_eq!(io.pending_events(), 1);
        let p = InputEvent::Pointer {
            x: 1,
            y: 2,
            buttons: POINTER_LEFT,
        };
        io.events.borrow_mut().clear();
        io.events.borrow_mut().extend([text, p]);
        assert_eq!(
            io.take_pointer(),
            Pointer::default(),
            "text in front holds the pointer"
        );
        assert!(!io.key_ready());
        assert_eq!(io.take_pointer().buttons, POINTER_LEFT, "then it comes");
        assert_eq!(io.handler_pointer().buttons, POINTER_LEFT);
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
    fn the_frame_version_moves_only_when_the_frame_is_composed() {
        // The first tick composes the frame; a tick where the game presented nothing and the
        // sprite stayed leaves it (and its version); a pointer move redraws the sprite; a
        // present publishes the screen.
        let mut e = idle_engine();
        assert_eq!(e.frame_version(), 0, "before the first tick");
        e.tick(&[], 20);
        assert_eq!(e.frame_version(), 1, "the first tick");
        for _ in 0..3 {
            e.tick(&[], 20);
        }
        assert_eq!(e.frame_version(), 1, "nothing changed");
        let at = |x| InputEvent::Pointer {
            x,
            y: 50,
            buttons: 0,
        };
        e.tick(&[at(100)], 20);
        assert_eq!(e.frame_version(), 2, "the sprite moved");
        e.tick(&[at(100)], 20);
        assert_eq!(e.frame_version(), 2, "the same place");
        let mut p = engine_with(|_state, io| async move {
            loop {
                io.present();
                io.vblank().await;
                io.vblank().await;
            }
        });
        let versions: Vec<u64> = (0..5)
            .map(|_| {
                p.tick(&[], 20);
                p.frame_version()
            })
            .collect();
        assert_eq!(versions, [1, 1, 2, 2, 3], "a present every other tick");
    }

    #[test]
    fn the_published_frame_is_the_screen_without_the_sprite() {
        // Port (spec 11 Design 7): the shown frame is the published one with the sprite drawn
        // over it; the published frame stays the screen as the game drew it, and its version
        // moves at a publish only, never for the sprite alone. The sprite's colours are the
        // shown frame's 16 + v.
        let mut e = idle_engine();
        e.tick(&[], 20);
        assert_eq!(
            (e.frame_version(), e.published_version()),
            (1, 0),
            "the first tick composes the shown frame; nothing was presented"
        );
        e.tick(
            &[InputEvent::Pointer {
                x: 100,
                y: 50,
                buttons: 0,
            }],
            20,
        );
        assert_eq!(e.frame_version(), 2, "the sprite moved: shown again");
        assert_eq!(e.published_version(), 0, "the published frame as it was");
        assert_eq!(e.sprite().map(|sp| (sp.x, sp.y)), Some((99, 48)));
        assert_eq!(
            e.frame().get(99, 48),
            Some(17),
            "the sprite in the shown frame"
        );
        assert_eq!(
            e.published_frame().get(99, 48),
            Some(0),
            "not in the published one"
        );
        assert_eq!(e.sprite_colour(1), Some(e.frame().palette[17]));
        assert_eq!(e.sprite_colour(0), None, "0 is clear");
        let mut p = engine_with(|_state, io| async move {
            loop {
                io.present();
                io.vblank().await;
                io.vblank().await;
            }
        });
        let versions: Vec<u64> = (0..5)
            .map(|_| {
                p.tick(&[], 20);
                p.published_version()
            })
            .collect();
        assert_eq!(versions, [1, 1, 2, 2, 3], "a present every other tick");
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

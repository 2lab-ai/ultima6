//! Platform-free Ultima VI game state and logic: the Amiga `game` ported per original function.
//!
//! Contract (every platform and every later layer, AI NPC drivers and mods included, relies on
//! it; spec `.prd/03-core-port.md`):
//!
//! - **In:** the platform builds [`Files`] from the user's disks (u6-intake), creates an
//!   [`Engine`], then once per host tick calls [`Engine::tick`] with the input events since the
//!   previous tick and the milliseconds elapsed.
//! - **Out:** [`Engine::frame`] is the last published 320×200 indexed frame with its palette;
//!   [`Engine::take_log`] / [`Engine::stub_hits`] report the stub points the game reached.
//!   [`Engine::take_speech`] is a channel of its own (spec 09): what NPCs say while the
//!   platform has voices on ([`Engine::set_speech`]), never in the log.
//! - This crate never performs I/O and never reads a clock: no files, network, environment,
//!   threads, `std::time`. Given the same files, seed, events and tick lengths it produces the
//!   same frames, so runs are replayable and testable headless. `clippy.toml` in this crate turns
//!   the common violations into lint errors.
//!
//! Layout (one directory per work unit, spec 03 §6): `engine` (executor, `Io`), `state`
//! (aggregate + loading), `rng`, `files`, `stub`, `pacer` (the hosts' fixed step and speed,
//! spec 06), then `gfx/`, `map/`, `objects/`, `ui/`, `actors/`, `converse/` and `main_loop`;
//! `hd` gives the HD compositor the images the source plane names (spec 07); `speech` keeps
//! what NPCs say for the platform's voices (spec 09); `locale` puts a pack's translation of the
//! conversation runs (spec 08).

pub mod actors;
pub mod combat;
pub mod converse;
pub mod engine;
pub mod files;
pub mod gfx;
pub mod hd;
pub mod image;
pub mod intro;
pub mod inventory;
pub mod locale;
pub mod magic;
pub mod main_loop;
pub mod map;
pub mod objects;
pub mod pacer;
pub mod rng;
pub mod saveload;
pub mod sound;
pub mod speech;
pub mod state;
pub mod stub;
pub mod ui;
pub mod usecmd;

pub use engine::{BootError, Engine, InputEvent, Io, Key};
pub use files::Files;
pub use rng::Rng;
pub use state::{LoadError, State};
pub use u6_render::{Framebuffer, HEIGHT, Kind, Rgb, Src, WIDTH};

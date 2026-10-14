//! Platform-free parsers for the **Amiga** Ultima VI data files.
//!
//! Spec: `re/notes/formats.md` (byte-level, measured on the Amiga files). Every parser takes the
//! file bytes (already PowerPacker-decrunched, see `u6-intake`) and returns plain owned data with
//! `pub` fields, or a [`FormatError`] carrying the offset of the problem. No I/O, no `unsafe`;
//! builds for `wasm32-unknown-unknown`. Enable the `serde` feature for Serialize/Deserialize.
#![forbid(unsafe_code)]

pub mod animdata;
pub mod basetile;
pub mod bitmap;
pub mod book;
mod bytes;
pub mod chunks;
pub mod converse;
pub mod ega;
pub mod error;
pub mod font;
pub mod gfx;
pub mod look;
pub mod lzw;
pub mod map;
pub mod objblk;
pub mod objlist;
pub mod portrait;
pub mod position;
pub mod schedule;
pub mod tileflag;
pub mod u6lib;

pub use error::{FormatError, Result};

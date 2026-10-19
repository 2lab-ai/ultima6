//! The local imagegen HD tool (spec `.prd/07-hd-art.md` HD2): builds the owner's HD pack from
//! the owner's own files with Codex's built-in image tool. It runs on the owner's machine only;
//! every output stays in the work dir outside the repository (see [`work`]).
//!
//! ```text
//! u6-hd inventory (--game PATH | --synthetic) [--work DIR]   counts by kind, printed locally
//! u6-hd atlas     (--game PATH | --synthetic) [--work DIR] [--rebuild]
//! u6-hd run       [--work DIR] [--send] [--limit N] [--parallel N] [--codex PATH]
//! u6-hd ingest    (--game PATH | --synthetic) [--work DIR] [--gate NAME=VALUE]...
//! u6-hd pack      [--work DIR] [--out FILE]
//! ```
//!
//! `--gate` changes one threshold of `gate::Thresholds` for the run (structure, shift, iou,
//! purity, seam, motion; the defaults are the spec's). `--game` takes the owner's zip, disk images or a folder of files (the page's intake reads
//! them). `--synthetic` takes the made-up set of [`game::synthetic`] instead, so the whole
//! pipeline (real imagegen calls included) can be run without any game art. `run` sends
//! nothing without `--send`, and exits non-zero when a call failed. There is no `export`: packs made from the game's art are derived
//! (`derived: true`) and are never shared.

pub mod atlas;
pub mod game;
pub mod gate;
pub mod group;
pub mod img;
pub mod ingest;
pub mod neighbour;
pub mod pack;
pub mod prompt;
pub mod run;
pub mod work;

#[cfg(test)]
mod tests;

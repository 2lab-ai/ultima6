//! Whether a dropped file is an HD pack (spec 07 HD1: the page classifies a drop before it starts
//! a game from it), told from the zip's central directory and its `manifest.json` only, read in
//! slices: the end of the file (from [`zip::tail_start`]), again from an earlier offset while the
//! central directory starts before what was read (at most `zip::MAX_DIRECTORY_BYTES`), then the
//! range of the root `manifest.json` entry, at most [`MAX_MANIFEST_RANGE`] bytes of it.
//!
//! A file is a pack when its zip directory reads and has a root `manifest.json`, and that entry
//! - reads (its local header, size and CRC-32 checked) as a JSON object whose `format` is
//!   [`FORMAT`] (the rest of the manifest is checked as the reader checks it, below); or
//! - does not read as one, being over the reader's limit for it (64 KiB), broken (its CRC-32,
//!   its size, its local header) or not a JSON object with a `format`, while its bytes as read
//!   name [`FORMAT`]: it says it
//!   is a pack, so it is taken for one (and refused) rather than started as a game. A manifest
//!   that does not read and does not name the format (a deflated one is searched as stored) is
//!   not a pack's.
//!
//! A zip without a root `manifest.json`, one whose manifest reads as JSON naming another format,
//! and anything whose directory does not read are not packs.
//!
//! For a pack the probe also makes the checks the reader makes before it reads any picture:
//! those of the directory (every entry's name one a pack has, none twice, what they declare
//! within the limit) and of the manifest; [`Probe::refusal`] is the first that fails, so the
//! page refuses such a pack without reading the file whole.

use u6_intake::zip::{self, Directory, Slice};

use crate::{FORMAT, MAX_MANIFEST, PackError, check_directory, check_manifest};

/// The most of a `manifest.json` entry's range read: the reader's limit for its data, its local
/// header and the longest local extra field a range may allow for.
pub const MAX_MANIFEST_RANGE: u64 = MAX_MANIFEST + (1 << 17);

/// One file's classification, read in slices: [`Probe::need`] names each range to read and
/// [`Probe::give`] takes its bytes, until [`Probe::is_pack`] says.
#[derive(Clone, Debug)]
pub struct Probe {
    size: u64,
    stage: Stage,
}

#[derive(Clone, Debug)]
enum Stage {
    /// Reading the file from `from` to its end.
    Directory {
        from: u64,
    },
    /// Reading the first `len` bytes of the `manifest.json` entry's range; `directory` is what
    /// the directory check said of the whole zip.
    Manifest {
        slice: Slice,
        len: u64,
        directory: Result<(), PackError>,
    },
    Done(Told),
}

/// What a probe told.
#[derive(Clone, Debug)]
enum Told {
    NotPack,
    Pack(Option<PackError>),
}

/// Whether `bytes` name the format: [`FORMAT`] somewhere in them, as they are.
fn names_format(bytes: &[u8]) -> bool {
    bytes.windows(FORMAT.len()).any(|w| w == FORMAT.as_bytes())
}

/// A manifest's `format` alone (any other field is let be).
#[derive(serde::Deserialize)]
struct Format {
    format: serde_json::Value,
}

/// Whether `bytes` is a manifest of this format: a JSON object whose `format` is [`FORMAT`].
pub fn is_manifest(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Format>(bytes).is_ok_and(|m| m.format == FORMAT)
}

impl Probe {
    /// A file of `size` bytes.
    pub fn new(size: u64) -> Probe {
        let stage = if size == 0 {
            Stage::Done(Told::NotPack)
        } else {
            Stage::Directory {
                from: zip::tail_start(size),
            }
        };
        Probe { size, stage }
    }

    /// The next range of the file to read, `start..end`, or `None` once it is told.
    pub fn need(&self) -> Option<(u64, u64)> {
        match &self.stage {
            Stage::Directory { from } => Some((*from, self.size)),
            Stage::Manifest { slice, len, .. } => Some((slice.start, slice.start + len)),
            Stage::Done(_) => None,
        }
    }

    /// Takes the bytes of the range [`Probe::need`] named; bytes that are not that range, or do
    /// not read as that part of a zip, tell it: not a pack.
    pub fn give(&mut self, bytes: &[u8]) {
        let Some((start, end)) = self.need() else {
            return;
        };
        if bytes.len() as u64 != end - start {
            self.stage = Stage::Done(Told::NotPack);
            return;
        }
        let stage = std::mem::replace(&mut self.stage, Stage::Done(Told::NotPack));
        self.stage = match stage {
            Stage::Directory { from } => self.directory(from, bytes),
            Stage::Manifest {
                slice, directory, ..
            } => Stage::Done(manifest(&slice, bytes, directory)),
            done @ Stage::Done(_) => done,
        };
    }

    /// The directory read from `from`: the next stage.
    fn directory(&self, from: u64, bytes: &[u8]) -> Stage {
        let found = zip::directory(self.size, from, bytes, |n| n == "manifest.json");
        let slice = match found {
            Ok(Directory::Need(at)) => return Stage::Directory { from: at },
            Ok(Directory::Entries(found)) => match found.into_iter().next() {
                Some(s) => s,
                None => return Stage::Done(Told::NotPack),
            },
            Err(_) => return Stage::Done(Told::NotPack),
        };
        // The same bytes walked for every entry: the reader's directory checks.
        let directory = match zip::directory(self.size, from, bytes, |_| true) {
            Ok(Directory::Entries(all)) => check_directory(&all).map(drop),
            Ok(Directory::Need(_)) => Err(PackError::Zip("directory outside the file".into())),
            Err(e) => Err(PackError::Zip(e.to_string())),
        };
        let len = slice.len().min(MAX_MANIFEST_RANGE);
        Stage::Manifest {
            slice,
            len,
            directory,
        }
    }

    /// Whether the file is a pack, once told (`None` while ranges are still needed).
    pub fn is_pack(&self) -> Option<bool> {
        match self.stage {
            Stage::Done(Told::Pack(_)) => Some(true),
            Stage::Done(Told::NotPack) => Some(false),
            _ => None,
        }
    }

    /// For a pack: the first check of the reader its directory or its manifest fails, so it is
    /// refused without reading it whole; `None` when they pass (the reader checks the rest) or
    /// for a file that is not a pack.
    pub fn refusal(&self) -> Option<&PackError> {
        match &self.stage {
            Stage::Done(Told::Pack(r)) => r.as_ref(),
            _ => None,
        }
    }
}

/// The manifest entry `s`, `bytes` the first of its range: a pack or not, and its refusal.
fn manifest(s: &Slice, bytes: &[u8], directory: Result<(), PackError>) -> Told {
    let over = s.unpacked() > MAX_MANIFEST || s.len() > MAX_MANIFEST_RANGE;
    let read = if over {
        Err(PackError::Limit(
            "manifest.json is larger than it can be".into(),
        ))
    } else {
        // The entry read from its own bytes: its range moved to the start.
        let mut at = s.clone();
        at.end -= at.start;
        at.start = 0;
        zip::entry(bytes, &at).map_err(|e| PackError::Zip(e.to_string()))
    };
    let checked = match &read {
        Ok(m) if is_manifest(m) => check_manifest(m).map(drop),
        Ok(m) if names_format(m) && serde_json::from_slice::<Format>(m).is_err() => {
            check_manifest(m).map(drop)
        }
        Ok(_) => return Told::NotPack,
        Err(e) if names_format(bytes) => Err(e.clone()),
        Err(_) => return Told::NotPack,
    };
    Told::Pack(directory.and(checked).err())
}

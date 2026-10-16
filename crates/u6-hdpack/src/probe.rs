//! Whether a dropped file is an HD pack (spec 07 HD1: the page classifies a drop before it starts
//! a game from it), told from the zip's central directory and its `manifest.json` only, read in
//! slices: the end of the file (from [`zip::tail_start`]), again from an earlier offset while the
//! central directory starts before what was read (at most `zip::MAX_DIRECTORY_BYTES`), then the
//! range of the root `manifest.json` entry (at most [`MAX_MANIFEST_RANGE`] bytes; a larger one is
//! not read).
//!
//! The rule: a file is a pack if and only if its zip directory reads, it has a root
//! `manifest.json` within the reader's limits for it (64 KiB unpacked, its range at most
//! [`MAX_MANIFEST_RANGE`]), that entry reads (its local header, size and CRC-32 checked), and
//! its data parses as a JSON object whose root `format` is the string [`FORMAT`]. Anything else
//! is not a pack and goes to the import as any dropped zip does, which takes only the disk
//! images in it: no root `manifest.json`, one too large, broken or unreadable, one that is not
//! JSON or not an object, one without a root `format` or with another (a format named deeper in
//! it included).
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
    /// Reading the `manifest.json` entry's range; `directory` is what the directory check said
    /// of the whole zip.
    Manifest {
        slice: Slice,
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

/// Whether a manifest's data makes a pack: it parses as a JSON object whose root `format` is
/// the string [`FORMAT`].
fn says_pack(data: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(data)
        .is_ok_and(|v| v.get("format").and_then(serde_json::Value::as_str) == Some(FORMAT))
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
            Stage::Manifest { slice, .. } => Some((slice.start, slice.end)),
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
            Stage::Manifest { slice, directory } => Stage::Done(manifest(&slice, bytes, directory)),
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
        // A manifest over the reader's limits does not read: not a pack, nothing more read.
        if slice.unpacked() > MAX_MANIFEST || slice.len() > MAX_MANIFEST_RANGE {
            return Stage::Done(Told::NotPack);
        }
        // The same bytes walked for every entry: the reader's directory checks.
        let directory = match zip::directory(self.size, from, bytes, |_| true) {
            Ok(Directory::Entries(all)) => check_directory(&all).map(drop),
            Ok(Directory::Need(_)) => Err(PackError::Zip("directory outside the file".into())),
            Err(e) => Err(PackError::Zip(e.to_string())),
        };
        Stage::Manifest { slice, directory }
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

/// The manifest entry `s`, `bytes` its range: a pack or not, and for a pack its refusal.
fn manifest(s: &Slice, bytes: &[u8], directory: Result<(), PackError>) -> Told {
    // The entry read from its own bytes: its range moved to the start.
    let mut at = s.clone();
    at.end -= at.start;
    at.start = 0;
    match zip::entry(bytes, &at) {
        Ok(m) if says_pack(&m) => Told::Pack(directory.and(check_manifest(&m).map(drop)).err()),
        _ => Told::NotPack,
    }
}

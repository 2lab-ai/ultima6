//! Whether a dropped file is an HD pack (spec 07 HD1: the page classifies a drop before it starts
//! a game from it), told from the zip's central directory and its `manifest.json` only, read in
//! slices: the end of the file (from [`zip::tail_start`]), again from an earlier offset while the
//! central directory starts before what was read (at most `zip::MAX_DIRECTORY_BYTES`), then the
//! `manifest.json` entry when it declares at most the reader's limit for it (64 KiB). It is a
//! pack when that entry reads (its local header, size and CRC-32 checked) as a JSON object whose
//! `format` is [`FORMAT`]; the rest of the manifest is not looked at here (the reader checks it,
//! so a pack with a wrong manifest is refused, never started as a game). Anything that does not
//! read is not a pack.

use u6_intake::zip::{self, Directory, Slice};

use crate::{FORMAT, MAX_MANIFEST};

/// The most of a `manifest.json` entry's range read: its data, its local header and the
/// longest local extra field a range may allow for.
const MAX_MANIFEST_RANGE: u64 = MAX_MANIFEST + (1 << 17);

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
    /// Reading the `manifest.json` entry.
    Manifest(Slice),
    Done(bool),
}

/// Whether `bytes` is a manifest of this format: a JSON object whose `format` is [`FORMAT`].
pub fn is_manifest(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .is_ok_and(|v| v.get("format").and_then(serde_json::Value::as_str) == Some(FORMAT))
}

impl Probe {
    /// A file of `size` bytes.
    pub fn new(size: u64) -> Probe {
        let stage = if size == 0 {
            Stage::Done(false)
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
            Stage::Manifest(s) => Some((s.start, s.end)),
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
            self.stage = Stage::Done(false);
            return;
        }
        self.stage = match &self.stage {
            Stage::Directory { from } => {
                match zip::directory(self.size, *from, bytes, |n| n == "manifest.json") {
                    Ok(Directory::Need(at)) => Stage::Directory { from: at },
                    Ok(Directory::Entries(found)) => match found.into_iter().next() {
                        Some(s) if s.full <= MAX_MANIFEST && s.len() <= MAX_MANIFEST_RANGE => {
                            Stage::Manifest(s)
                        }
                        _ => Stage::Done(false),
                    },
                    Err(_) => Stage::Done(false),
                }
            }
            Stage::Manifest(s) => {
                // The entry read from its own bytes: its range moved to the start.
                let mut at = s.clone();
                at.end -= at.start;
                at.start = 0;
                Stage::Done(zip::entry(bytes, &at).is_ok_and(|m| is_manifest(&m)))
            }
            Stage::Done(told) => Stage::Done(*told),
        };
    }

    /// Whether the file is a pack, once told (`None` while ranges are still needed).
    pub fn is_pack(&self) -> Option<bool> {
        match self.stage {
            Stage::Done(told) => Some(told),
            _ => None,
        }
    }
}

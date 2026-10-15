//! The dropped zips: the entries the intake takes from a zip (the filter of `u6_intake`'s
//! `unzip_disks`: `.hfe` / `.adf`, no `__MACOSX/`, no `._` files), what reading those yields
//! at most, read from the central directory without inflating anything
//! ([`crate::MAX_UNZIPPED_BYTES`]; a zip whose directory does not read is refused before the
//! intake runs, [`declared_disk_bytes`]), and a large zip read in slices ([`Sliced`]).

use u6_intake::zip::{self, Directory, MAX_ENTRIES, Rebuilt, Slice};

pub(crate) fn ends_with_ci(name: &str, suffix: &str) -> bool {
    let (n, s) = (name.as_bytes(), suffix.as_bytes());
    n.len() >= s.len() && n[n.len() - s.len()..].eq_ignore_ascii_case(s)
}

/// The entries the intake inflates.
pub(crate) fn is_disk_entry(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    (ends_with_ci(name, ".hfe") || ends_with_ci(name, ".adf"))
        && !name.starts_with("__MACOSX/")
        && !base.starts_with("._")
}

/// What reading the entries `slices` yields at most, together: a stored entry its data, a
/// deflated one what it declares unpacked (inflating stops there).
fn unpacked(slices: &[Slice]) -> u64 {
    slices
        .iter()
        .fold(0, |sum, s| sum.saturating_add(s.unpacked()))
}

/// The line for the dropped zip `name` that does not read (as the intake words it).
fn unreadable(name: &str, err: impl std::fmt::Display) -> String {
    format!("{name}: zip unreadable: {err}")
}

/// What the zip readers say of bytes that are not the zip's they claim to be.
const INVALID: &str = "invalid zip data";

/// What the disk-image entries of the zip `data` yield at most when read; `Ok(None)` when `data`
/// is not a zip, `Err` (the zip-unreadable line) when it is one whose directory or disk-image
/// entries do not read (the intake would refuse it too, see `u6_intake::zip`).
pub fn declared_disk_bytes(name: &str, data: &[u8]) -> Result<Option<u64>, String> {
    if !zip::is_zip(data) {
        return Ok(None);
    }
    match zip::directory(data.len() as u64, 0, data, is_disk_entry) {
        Ok(Directory::Entries(slices)) => Ok(Some(unpacked(&slices))),
        Ok(Directory::Need(_)) => Err(unreadable(name, INVALID)),
        Err(err) => Err(unreadable(name, err)),
    }
}

/// A dropped file the page reads in slices: the page reads each range [`Sliced::need`] names and
/// hands its bytes to [`Sliced::give`]. First its first four bytes: a file that does not start
/// as a zip ([`zip::is_zip`], as for a file read whole) is then read whole and collected as it
/// is. A zip: the end of the file, again from an earlier offset while the central directory
/// starts before what was read, then each disk-image entry, checked against its central record;
/// once all are in, [`Sliced::finish`] is a zip of only those entries, which the intake reads as
/// it would read them in the dropped zip.
pub(crate) struct Sliced {
    pub(crate) path: String,
    size: u64,
    stage: Stage,
}

enum Stage {
    /// Reading the first bytes, to tell a zip.
    Head,
    /// Not a zip: reading it whole, then holding it.
    Whole,
    Held(Vec<u8>),
    /// Reading the file from `from` to its end.
    Directory {
        from: u64,
    },
    /// The disk-image entries, the next to read and the zip assembled from those read.
    Entries {
        wanted: Vec<Slice>,
        next: usize,
        rebuilt: Rebuilt,
    },
}

impl Sliced {
    pub(crate) fn new(path: &str, size: u64) -> Self {
        Sliced {
            path: path.to_string(),
            size,
            stage: Stage::Head,
        }
    }

    /// The next range of the file to read, `start..end`, or `None` once everything is in.
    pub(crate) fn need(&self) -> Option<(u64, u64)> {
        match &self.stage {
            Stage::Head => Some((0, self.size.min(4))),
            Stage::Whole => Some((0, self.size)),
            Stage::Held(_) => None,
            Stage::Directory { from } => Some((*from, self.size)),
            Stage::Entries { wanted, next, .. } => wanted.get(*next).map(|s| (s.start, s.end)),
        }
    }

    /// Takes the bytes of the range [`Sliced::need`] named. `held`: what the dropped files hold
    /// in wasm with these bytes; once the directory is read, its disk-image entries must be at
    /// most [`MAX_ENTRIES`], yield at most [`crate::MAX_UNZIPPED_BYTES`], and the zip of them
    /// (their ranges, read into it, and its central directory: [`Rebuilt::holds`]) fit under
    /// [`crate::MAX_INPUT_BYTES`] with `held`, or none of them is read and nothing reserved.
    pub(crate) fn give(&mut self, bytes: Vec<u8>, held: usize) -> Result<(), String> {
        let Some((start, end)) = self.need() else {
            return Err(format!("{}: no more of it was asked for", self.path));
        };
        if bytes.len() as u64 != end - start {
            return Err(unreadable(&self.path, INVALID));
        }
        let path = &self.path;
        match &mut self.stage {
            Stage::Head if zip::is_zip(&bytes) => {
                self.stage = Stage::Directory {
                    from: zip::tail_start(self.size),
                };
            }
            Stage::Head => self.stage = Stage::Whole,
            Stage::Whole => self.stage = Stage::Held(bytes),
            Stage::Held(_) => unreachable!("need() names nothing once held"),
            Stage::Directory { from } => {
                let read = zip::directory(self.size, *from, &bytes, is_disk_entry);
                match read.map_err(|err| unreadable(path, err))? {
                    Directory::Need(at) => *from = at,
                    Directory::Entries(wanted) => {
                        if wanted.len() as u64 > MAX_ENTRIES {
                            return Err(unreadable(path, INVALID));
                        }
                        let yields = unpacked(&wanted);
                        if yields > crate::MAX_UNZIPPED_BYTES {
                            return Err(crate::unpacks_too_much(yields));
                        }
                        let total = (held as u64).saturating_add(Rebuilt::holds(&wanted));
                        if total > crate::MAX_INPUT_BYTES as u64 {
                            return Err(crate::too_large(total));
                        }
                        self.stage = Stage::Entries {
                            rebuilt: Rebuilt::for_slices(&wanted),
                            wanted,
                            next: 0,
                        };
                    }
                }
            }
            Stage::Entries {
                wanted,
                next,
                rebuilt,
            } => {
                let added = rebuilt.add(&wanted[*next], &bytes);
                added.map_err(|err| unreadable(path, err))?;
                *next += 1;
            }
        }
        Ok(())
    }

    /// The file to collect once [`Sliced::need`] has nothing more to read: the zip of the
    /// disk-image entries read, or the file read whole.
    pub(crate) fn finish(self) -> Result<Vec<u8>, String> {
        match self.stage {
            Stage::Held(bytes) => Ok(bytes),
            Stage::Entries {
                wanted,
                next,
                rebuilt,
            } if next == wanted.len() => rebuilt.finish().map_err(|e| unreadable(&self.path, e)),
            _ => Err(format!("{}: the file is not read yet", self.path)),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// One entry of a synthetic zip: its name, method, the packed and unpacked sizes its records
    /// declare, and the bytes written as its data (which may be shorter than declared).
    #[derive(Clone, Copy)]
    pub(crate) struct Entry<'a> {
        pub(crate) name: &'a str,
        pub(crate) method: u16,
        pub(crate) csize: u32,
        pub(crate) full: u32,
        pub(crate) body: &'a [u8],
    }

    /// A stored entry holding `body`.
    pub(crate) fn stored<'a>(name: &'a str, body: &'a [u8]) -> Entry<'a> {
        let n = body.len() as u32;
        Entry {
            name,
            method: 0,
            csize: n,
            full: n,
            body,
        }
    }

    /// A synthetic zip of `entries` (no extra fields, no comment, CRCs 0).
    pub(crate) fn zip(entries: &[Entry]) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        for e in entries {
            let local = out.len() as u32;
            le32(&mut out, 0x0403_4b50);
            le16(&mut out, 20);
            le16(&mut out, 0);
            le16(&mut out, e.method);
            le32(&mut out, 0); // time, date
            le32(&mut out, 0); // CRC
            le32(&mut out, e.csize);
            le32(&mut out, e.full);
            le16(&mut out, e.name.len() as u16);
            le16(&mut out, 0);
            out.extend_from_slice(e.name.as_bytes());
            out.extend_from_slice(e.body);
            le32(&mut central, 0x0201_4b50);
            le16(&mut central, 20);
            le16(&mut central, 20);
            le16(&mut central, 0);
            le16(&mut central, e.method);
            le32(&mut central, 0);
            le32(&mut central, 0);
            le32(&mut central, e.csize);
            le32(&mut central, e.full);
            le16(&mut central, e.name.len() as u16);
            central.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
            le32(&mut central, local);
            central.extend_from_slice(e.name.as_bytes());
        }
        let at = out.len() as u32;
        let n = entries.len() as u16;
        out.extend_from_slice(&central);
        le32(&mut out, 0x0605_4b50);
        le32(&mut out, 0);
        le16(&mut out, n);
        le16(&mut out, n);
        le32(&mut out, central.len() as u32);
        le32(&mut out, at);
        le16(&mut out, 0);
        out
    }

    #[test]
    fn sums_what_reading_the_disk_entries_yields_at_most() {
        let deflated = Entry {
            method: 8,
            full: 2_000_000,
            ..stored("d/Disk 1.HFE", b"xx")
        };
        let z = zip(&[
            deflated,
            stored("scan.jxl", &[1; 900]),
            stored("__MACOSX/d/Disk 1.hfe", &[2; 70]),
            stored("d/._Disk 2.adf", &[3; 70]),
            Entry {
                full: 70_000_000, // stored: its data is what reading it yields
                ..stored("Disk 2.adf", &[4; 1200])
            },
        ]);
        assert_eq!(declared_disk_bytes("u6.zip", &z), Ok(Some(2_001_200)));
    }

    #[test]
    fn not_a_zip_is_none_and_a_zip_that_does_not_read_is_refused() {
        assert_eq!(declared_disk_bytes("x", b"PP20...."), Ok(None));
        assert_eq!(declared_disk_bytes("x", &zip(&[])), Ok(Some(0)));
        let cut = declared_disk_bytes("u6.zip", b"PK\x03\x04 truncated");
        assert_eq!(cut, Err("u6.zip: zip unreadable: invalid zip data".into()));
    }
}

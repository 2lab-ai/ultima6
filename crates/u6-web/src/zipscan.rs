//! The dropped zips: the entries the intake takes from a zip (the filter of `u6_intake`'s
//! `unzip_disks`: `.hfe` / `.adf`, no `__MACOSX/`, no `._` files), what those declare
//! uncompressed, read from the central directory without inflating anything
//! ([`crate::MAX_UNZIPPED_BYTES`]; an archive whose directory does not read is left to the
//! intake, which reports it), and a large zip read in slices ([`Sliced`]).

use u6_intake::zip::{self, Directory, Rebuilt, Slice};

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

/// What the entries `slices` declare uncompressed, together.
fn declared(slices: &[Slice]) -> u64 {
    slices.iter().fold(0, |sum, s| sum.saturating_add(s.full))
}

/// The sum of the declared uncompressed sizes of the disk-image entries of the zip `data`, or
/// `None` when `data` is not a zip whose directory reads.
pub fn declared_disk_bytes(data: &[u8]) -> Option<u64> {
    if !zip::is_zip(data) {
        return None;
    }
    match zip::directory(data.len() as u64, 0, data, is_disk_entry).ok()? {
        Directory::Entries(slices) => Some(declared(&slices)),
        Directory::Need(_) => None,
    }
}

/// A dropped zip read in slices: the page reads each range [`Sliced::need`] names and hands its
/// bytes to [`Sliced::give`] (first the end of the file, from an earlier offset again while the
/// central directory starts before what was read, then each disk-image entry, checked against
/// its central record); once all are in, [`Sliced::finish`] is a zip of only those entries,
/// which the intake reads as it would read them in the dropped zip.
pub(crate) struct Sliced {
    pub(crate) path: String,
    size: u64,
    stage: Stage,
}

enum Stage {
    /// Reading the file from `from` to its end.
    Directory { from: u64 },
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
            stage: Stage::Directory {
                from: zip::tail_start(size),
            },
        }
    }

    /// The next range of the file to read, `start..end`, or `None` once every entry is in.
    pub(crate) fn need(&self) -> Option<(u64, u64)> {
        match &self.stage {
            Stage::Directory { from } => Some((*from, self.size)),
            Stage::Entries { wanted, next, .. } => wanted.get(*next).map(|s| (s.start, s.end)),
        }
    }

    /// Takes the bytes of the range [`Sliced::need`] named. `held`: what the dropped files hold
    /// in wasm with these bytes; once the directory is read, its disk-image entries must declare
    /// at most [`crate::MAX_UNZIPPED_BYTES`] unpacked and their ranges fit under
    /// [`crate::MAX_INPUT_BYTES`] with `held`, or none of them is read.
    pub(crate) fn give(&mut self, bytes: &[u8], held: usize) -> Result<(), String> {
        let unreadable = |err: u6_intake::Error| format!("{}: zip unreadable: {err}", self.path);
        match &mut self.stage {
            Stage::Directory { from } => {
                match zip::directory(self.size, *from, bytes, is_disk_entry).map_err(unreadable)? {
                    Directory::Need(at) => *from = at,
                    Directory::Entries(wanted) => {
                        let unpacked = declared(&wanted);
                        if unpacked > crate::MAX_UNZIPPED_BYTES {
                            return Err(crate::unpacks_too_much(unpacked));
                        }
                        let room = wanted
                            .iter()
                            .fold(0u64, |sum, s| sum.saturating_add(s.len()));
                        let total = (held as u64).saturating_add(room);
                        if total > crate::MAX_INPUT_BYTES as u64 {
                            return Err(crate::too_large(total));
                        }
                        self.stage = Stage::Entries {
                            rebuilt: Rebuilt::with_capacity(room as usize),
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
                let Some(slice) = wanted.get(*next) else {
                    return Err(format!("{}: no more of the zip was asked for", self.path));
                };
                rebuilt.add(slice, bytes).map_err(unreadable)?;
                *next += 1;
            }
        }
        Ok(())
    }

    /// The zip of the disk-image entries read, once [`Sliced::need`] has nothing more to read.
    pub(crate) fn finish(self) -> Result<Vec<u8>, String> {
        let unreadable = |err: u6_intake::Error| format!("{}: zip unreadable: {err}", self.path);
        match self.stage {
            Stage::Entries {
                wanted,
                next,
                rebuilt,
            } if next == wanted.len() => rebuilt.finish().map_err(unreadable),
            _ => Err(format!("{}: the zip is not read yet", self.path)),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const EOCD: u32 = 0x0605_4b50;
    const CENTRAL: u32 = 0x0201_4b50;

    /// A stored zip of `entries` (name, declared size, bytes) with an optional Zip64 size.
    pub(crate) fn zip(entries: &[(&str, u32, &[u8])], zip64: Option<u64>) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for &(name, size, body) in entries {
            let local = out.len() as u32;
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[0; 14]);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(body);
            let extra: Vec<u8> = match zip64 {
                Some(v) if size == u32::MAX => {
                    let mut x = vec![1, 0, 8, 0];
                    x.extend_from_slice(&v.to_le_bytes());
                    x
                }
                _ => Vec::new(),
            };
            central.extend_from_slice(&CENTRAL.to_le_bytes());
            central.extend_from_slice(&[0; 16]);
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&size.to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&(extra.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 10]);
            central.extend_from_slice(&local.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
            central.extend_from_slice(&extra);
        }
        let at = out.len() as u32;
        let n = entries.len() as u16;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn sums_what_the_disk_entries_declare() {
        let z = zip(
            &[
                ("d/Disk 1.HFE", 2_000_000, b"x"),
                ("scan.jxl", 900_000_000, b"y"),
                ("__MACOSX/d/Disk 1.hfe", 70_000_000, b"z"),
                ("d/._Disk 2.adf", 70_000_000, b"z"),
                ("Disk 2.adf", 901_120, b"w"),
            ],
            None,
        );
        assert_eq!(declared_disk_bytes(&z), Some(2_901_120));
        let big = zip(&[("Disk.adf", u32::MAX, b"q")], Some(5 << 30));
        assert_eq!(declared_disk_bytes(&big), Some(5 << 30), "Zip64 size");
    }

    #[test]
    fn not_a_zip_is_none() {
        assert_eq!(declared_disk_bytes(b"PP20...."), None);
        assert_eq!(declared_disk_bytes(b"PK\x03\x04 truncated"), None);
        assert_eq!(declared_disk_bytes(&zip(&[], None)), Some(0));
    }
}

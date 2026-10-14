//! Platform-free intake: whatever the user drops -> Ultima VI Amiga disk images and files.
//!
//! ```text
//! zip (archive.org ultima-vi.zip) -> *.hfe / *.adf entries
//! HFE -> Amiga MFM -> ADF (901,120 bytes)
//! ADF -> AmigaDOS OFS/FFS -> "VOLUME/path" files
//! any file starting with "PP20" -> PowerPacker decrunch (checked per file)
//! ```
//!
//! Loose files (already extracted) pass through with PP20 decrunch. Rust port of
//! the former TypeScript intake in `web/` (removed after parity) (same pipeline, same report lines). No I/O, no clocks: pure
//! functions over byte slices, so it runs unchanged on native and
//! wasm32-unknown-unknown. `clippy.toml` in this crate turns the common violations
//! into lint errors.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

pub mod hfe;
pub mod mfm;
pub mod ofs;
pub mod pp20;
pub mod zip;

pub use hfe::{HfeHeader, HfeImage, is_hfe, parse_hfe};
pub use mfm::{ADF_SIZE, DiskDecode, TrackDecode, decode_amiga_disk, decode_amiga_track};
pub use ofs::{OfsVolume, adf_dos_type, read_ofs};
pub use pp20::{decrunch_pp20, is_pp20};

/// A decoding failure; `Display` is the text that lands in [`Intake::report`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Error(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// One dropped file: its name (or relative path) and bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputFile {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    /// Source name: the dropped file or the zip entry, e.g. "Ultima VI Disk 1.hfe".
    pub label: String,
    /// AmigaDOS volume name read from the root block, or `None` if unreadable.
    pub volume: Option<String>,
    pub adf: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Intake {
    /// Disk images in label order (numeric-aware: "Disk 2" before "Disk 10").
    pub disks: Vec<Disk>,
    /// `"VOLUME/dir/file"` (or the loose file's own path) -> bytes, PP20 already decrunched.
    pub files: BTreeMap<String, Vec<u8>>,
    pub report: Vec<String>,
}

fn ends_with_ci(name: &str, suffix: &str) -> bool {
    let (n, s) = (name.as_bytes(), suffix.as_bytes());
    n.len() >= s.len() && n[n.len() - s.len()..].eq_ignore_ascii_case(s)
}

fn is_disk_name(name: &str) -> bool {
    ends_with_ci(name, ".hfe") || ends_with_ci(name, ".adf")
}

fn is_adf(name: &str, data: &[u8]) -> bool {
    if adf_dos_type(data).is_none() {
        return false;
    }
    data.len() == ADF_SIZE || data.len() == ADF_SIZE * 2 || ends_with_ci(name, ".adf")
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Backslashes to slashes, then strip any run of leading "./" and "/".
fn normalize_path(name: &str) -> String {
    let s = name.replace('\\', "/");
    let mut rest = s.as_str();
    loop {
        if let Some(r) = rest.strip_prefix("./") {
            rest = r;
        } else if let Some(r) = rest.strip_prefix('/') {
            rest = r;
        } else {
            return rest.to_string();
        }
    }
}

/// Approximates the TS `localeCompare(b, "en", { numeric: true })`: digit runs compare
/// by value, other characters case-insensitively with whitespace < punctuation <
/// digits < letters; exact ties fall back to code point order.
fn by_label(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        let ord = match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let (m, n) = (digit_run(&mut x), digit_run(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                m.len().cmp(&n.len()).then_with(|| m.cmp(n))
            }
            (Some(c), Some(d)) => {
                x.next();
                y.next();
                char_key(c).cmp(&char_key(d))
            }
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
}

fn digit_run(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    std::iter::from_fn(|| it.next_if(char::is_ascii_digit)).collect()
}

fn char_key(c: char) -> (u8, char) {
    let class = if c.is_whitespace() {
        0
    } else if c.is_numeric() {
        2
    } else if c.is_alphabetic() {
        3
    } else {
        1
    };
    (class, c.to_lowercase().next().unwrap_or(c))
}

/// Insertion-ordered path -> bytes (TS `Map` semantics: a repeated path keeps its
/// first position, takes the later bytes, and is reported).
#[derive(Default)]
struct Collected {
    entries: Vec<(String, Vec<u8>)>,
    index: BTreeMap<String, usize>,
}

impl Collected {
    fn put(&mut self, path: String, bytes: Vec<u8>, report: &mut Vec<String>) {
        match self.index.get(&path) {
            Some(&i) => {
                report.push(format!("{path}: duplicate path, later copy kept"));
                self.entries[i].1 = bytes;
            }
            None => {
                self.index.insert(path.clone(), self.entries.len());
                self.entries.push((path, bytes));
            }
        }
    }
}

/// Pull disk images out of a zip; everything else in it (scans, flux streams) is ignored.
fn unzip_disks(name: &str, data: &[u8], report: &mut Vec<String>) -> Result<Vec<InputFile>, Error> {
    let entries = zip::read_zip(data, |entry| {
        is_disk_name(entry)
            && !entry.starts_with("__MACOSX/")
            && !base_name(entry).starts_with("._")
    })?;
    let out: Vec<InputFile> = entries
        .into_iter()
        .map(|(entry, data)| InputFile {
            name: base_name(&entry).to_string(),
            data,
        })
        .collect();
    let names: Vec<&str> = out.iter().map(|f| f.name.as_str()).collect();
    let list = if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    };
    report.push(format!("{name}: zip, {} disk image(s): {list}", out.len()));
    Ok(out)
}

/// HFE -> ADF with its report lines, or `None` (reported) when the container is unreadable.
fn hfe_to_adf(label: &str, data: &[u8], report: &mut Vec<String>) -> Option<Vec<u8>> {
    let hfe = match parse_hfe(data) {
        Ok(hfe) => hfe,
        Err(err) => {
            report.push(format!("{label}: HFE unreadable: {err}"));
            return None;
        }
    };
    let h = &hfe.header;
    let dec = decode_amiga_disk(&hfe);
    report.push(format!(
        "{label}: HFE {} cyl x {} sides, {} kbps, encoding {}; sectors {}/{} good, bad header checksums {}, \
         bad data checksums {}, wrong-track headers {}",
        h.cylinders,
        h.sides,
        h.bit_rate_kbps,
        h.track_encoding,
        dec.found,
        dec.total,
        dec.bad_header,
        dec.bad_data,
        dec.wrong_track,
    ));
    if !dec.missing.is_empty() {
        let shown: Vec<String> = dec.missing.iter().take(32).map(usize::to_string).collect();
        let more = if dec.missing.len() > 32 { ",..." } else { "" };
        report.push(format!(
            "{label}: missing blocks {}{more} (zero-filled)",
            shown.join(",")
        ));
    }
    Some(dec.adf)
}

/// Run the whole pipeline over the dropped files. Never fails: every problem becomes
/// a line in [`Intake::report`] and the affected input is skipped or kept raw.
pub fn intake(files: Vec<InputFile>) -> Intake {
    let mut report = Vec::new();
    let mut disk_inputs: Vec<InputFile> = Vec::new();
    let mut loose: Vec<InputFile> = Vec::new();

    for f in files {
        if zip::is_zip(&f.data) {
            match unzip_disks(&f.name, &f.data, &mut report) {
                Ok(found) => disk_inputs.extend(found),
                Err(err) => report.push(format!("{}: zip unreadable: {err}", f.name)),
            }
        } else if is_hfe(&f.data) || is_adf(&f.name, &f.data) {
            disk_inputs.push(f);
        } else {
            loose.push(f);
        }
    }
    disk_inputs.sort_by(|a, b| by_label(&a.name, &b.name));

    let mut disks = Vec::new();
    let mut raw = Collected::default();
    for f in disk_inputs {
        let adf = if is_hfe(&f.data) {
            match hfe_to_adf(&f.name, &f.data, &mut report) {
                Some(adf) => adf,
                None => continue,
            }
        } else {
            report.push(format!("{}: ADF {} bytes", f.name, f.data.len()));
            f.data
        };

        let volume = match read_ofs(&adf) {
            Ok(vol) => {
                let count = vol.files.len();
                for (path, bytes) in vol.files {
                    raw.put(format!("{}/{path}", vol.name), bytes, &mut report);
                }
                report.push(format!(
                    "{}: volume {} ({}), {count} files",
                    f.name, vol.name, vol.dos_type
                ));
                for w in &vol.warnings {
                    report.push(format!("{}: {w}", f.name));
                }
                Some(vol.name)
            }
            Err(err) => {
                report.push(format!("{}: filesystem unreadable: {err}", f.name));
                None
            }
        };
        disks.push(Disk {
            label: f.name,
            volume,
            adf,
        });
    }

    for f in loose {
        raw.put(normalize_path(&f.name), f.data, &mut report);
    }

    let mut out = BTreeMap::new();
    let (mut packed, mut failed) = (0usize, 0usize);
    for (path, bytes) in raw.entries {
        if !is_pp20(&bytes) {
            out.insert(path, bytes);
            continue;
        }
        match decrunch_pp20(&bytes) {
            Ok(plain) => {
                out.insert(path, plain);
                packed += 1;
            }
            Err(err) => {
                report.push(format!(
                    "{path}: PP20 decrunch failed, kept crunched: {err}"
                ));
                out.insert(path, bytes);
                failed += 1;
            }
        }
    }
    let failed_note = if failed > 0 {
        format!(", {failed} failed")
    } else {
        String::new()
    };
    report.push(format!(
        "PP20: decrunched {packed} file(s){failed_note}; {} file(s) total",
        out.len()
    ));

    Intake {
        disks,
        files: out,
        report,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_sort_numerically_and_case_insensitively() {
        let mut v = vec![
            "Disk 10.hfe",
            "disk 2.hfe",
            "Disk 1.hfe",
            "Disk 02.hfe",
            "Disk A.adf",
            "Disk.adf",
        ];
        v.sort_by(|a, b| by_label(a, b));
        assert_eq!(
            v,
            [
                "Disk 1.hfe",
                "Disk 02.hfe",
                "disk 2.hfe",
                "Disk 10.hfe",
                "Disk A.adf",
                "Disk.adf"
            ]
        );
    }

    #[test]
    fn paths_normalize_like_the_ts_intake() {
        assert_eq!(normalize_path("DATA1\\map"), "DATA1/map");
        assert_eq!(normalize_path("././/./x/./y"), "x/./y");
        assert_eq!(base_name("a/b/c.hfe"), "c.hfe");
        assert_eq!(base_name("c.hfe"), "c.hfe");
        assert!(is_disk_name("X.HFE") && is_disk_name("y.adf") && !is_disk_name("z.adf.txt"));
    }
}

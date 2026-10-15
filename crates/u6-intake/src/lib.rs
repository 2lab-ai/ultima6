//! Platform-free intake: whatever the user drops -> Ultima VI Amiga disk images and files.
//!
//! ```text
//! zip (archive.org ultima-vi.zip) -> *.hfe / *.adf entries
//! HFE -> Amiga MFM -> ADF (901,120 bytes)
//! ADF -> AmigaDOS OFS/FFS -> "VOLUME/path" files
//! any file starting with "PP20" -> PowerPacker decrunch (checked per file)
//! ```
//!
//! The intake holds at most [`INTAKE_BUDGET`] bytes of output: past it, it stops, says so and
//! keeps nothing.
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

pub use hfe::{HfeHeader, HfeImage, hfe_track_bytes, is_hfe, parse_hfe};
pub use mfm::{ADF_SIZE, DiskDecode, TrackDecode, decode_amiga_disk, decode_amiga_track};
pub use ofs::{OfsVolume, adf_dos_type, read_ofs, read_ofs_within};
pub use pp20::{decrunch_pp20, decrunch_pp20_within, is_pp20, pp20_declared_len};

/// The bytes the intake may hold in all (64 MiB).
///
/// Retained, charged before it is allocated and given back when the intake drops it. Strings
/// are built at their exact length, so a charge is what the allocation holds:
/// - each ADF it keeps: a dropped one as it is, an HFE's decoded one; while an HFE decodes, also
///   the tracks the parser copies out ([`hfe_track_bytes`]) and the decode's working bytes
///   ([`mfm::DECODE_WORK_BYTES`]: one track's cells, its sectors, the missing list);
/// - each volume's files as reserved, their paths (twice, as key and index, with room for the
///   volume name), the directory paths and the listed warnings, each with
///   [`ofs::ENTRY_BYTES`] for its list and map entries ([`OfsVolume::held`]);
/// - each loose file it keeps, and each PP20 output (its crunched bytes given back once done).
///
/// Spent, charged and never given back, so the work an image can cause is bounded too: the bytes
/// a failed file read reserved, and a block for each extension table a read walks.
///
/// Transient or outside it, not charged, each bounded apart from it:
/// - one warning or error string while it is made, before `warn` lists or counts it: a path
///   (at most [`ofs::MAX_PATH_BYTES`], 3,964 bytes) and a few words, reserved at most twice;
/// - the walk's directory paths along its recursion: at most [`ofs::MAX_DEPTH`] of at most
///   3,903 bytes, 249,792 bytes;
/// - the visited set's entries for blocks the walk keeps nothing for (one per block, of an ADF
///   already charged; a kept entry's is in [`ofs::ENTRY_BYTES`]);
/// - an HFE's track table as read before its charge: at most 255 entries of 16 bytes;
/// - the inputs and their names (the caller's), the zip entries `zip.rs` inflates, the report
///   lines that name an input, and per volume two fixed lines past its
///   [`ofs::MAX_WARNINGS`] warnings.
///
/// The size, from the formats, not from any game file: the game is three Amiga DD disks of
/// [`ADF_SIZE`] (901,120) bytes, 2,703,360 bytes of ADF. A volume's files fit in its blocks, at
/// most 2,703,360 bytes on the three, and PP20 output is at most 56/3 of its crunched stream
/// ([`pp20::MAX_EXPANSION`]), 50,462,720 bytes. While a file decrunches, its crunched bytes
/// (at most one disk, 901,120) are held as well, and the extension tables of 1,757 data blocks
/// cost 25 blocks a disk (38,400 bytes). The entries are at most 1,757 headers and 32 warnings
/// a disk, 5,367 on three; with every name at the root (at most 60 bytes) each is charged
/// 256 + 2 x (61 + 60) = 498 bytes, 2,672,766 in all. That is 56,778,366 bytes (54.1 MiB),
/// and 64 MiB is 18.2% above it (the same 64 MiB the web page allows the game files). Deeper
/// names cost more: a set of three disks fits while its paths average at most 1,022 bytes
/// (about 16 levels of 60-byte names); past that the intake stops at the budget, it does not
/// pass it. The HFE decodes (at most an HFE's size and 290,552 bytes) are given back before
/// any PP20 output.
pub const INTAKE_BUDGET: usize = 64 << 20;

const BUDGET_MIB: usize = INTAKE_BUDGET >> 20;

/// What is left of [`INTAKE_BUDGET`]. The first charge that does not fit spends it: every later
/// one fails too, so the intake stops expanding there.
struct Budget {
    left: usize,
    spent: bool,
}

impl Budget {
    /// Charges `n` bytes for the input `name`, or returns its report line when they do not fit.
    fn charge(&mut self, name: &str, n: usize) -> Result<(), String> {
        if self.spent {
            return Err(format!(
                "{name}: not read: the intake's {BUDGET_MIB} MiB budget is spent"
            ));
        }
        if n > self.left {
            self.spent = true;
            return Err(format!(
                "{name}: not read: {n} more bytes would pass the intake's {BUDGET_MIB} MiB budget"
            ));
        }
        self.left -= n;
        Ok(())
    }

    /// Charges bytes the caller already bounded by `left` (a volume read within it, a decrunch
    /// refused past it).
    fn take_within(&mut self, n: usize) {
        debug_assert!(n <= self.left, "charged past the budget");
        self.left = self.left.saturating_sub(n);
    }

    fn give_back(&mut self, n: usize) {
        self.left = self.left.saturating_add(n).min(INTAKE_BUDGET);
    }
}

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
    /// Returns the length of the earlier copy it drops (0 for a new path).
    fn put(&mut self, path: String, bytes: Vec<u8>, report: &mut Vec<String>) -> usize {
        match self.index.get(&path) {
            Some(&i) => {
                report.push(format!("{path}: duplicate path, later copy kept"));
                std::mem::replace(&mut self.entries[i].1, bytes).len()
            }
            None => {
                self.index.insert(path.clone(), self.entries.len());
                self.entries.push((path, bytes));
                0
            }
        }
    }
}

/// `a`, `sep`, `b` in one string at its exact length (`format!` reserves about twice).
fn joined(a: &str, sep: &str, b: &str) -> String {
    let mut s = String::with_capacity(a.len() + sep.len() + b.len());
    s.push_str(a);
    s.push_str(sep);
    s.push_str(b);
    s
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
/// Charges the ADF, the track copy and the decode's working bytes before it parses, and gives
/// back all but the ADF once the decode is done.
fn hfe_to_adf(
    label: &str,
    data: &[u8],
    budget: &mut Budget,
    report: &mut Vec<String>,
) -> Option<Vec<u8>> {
    let work = match hfe_track_bytes(data) {
        Ok(tracks) => tracks + mfm::DECODE_WORK_BYTES,
        Err(err) => {
            report.push(format!("{label}: HFE unreadable: {err}"));
            return None;
        }
    };
    if let Err(line) = budget.charge(label, ADF_SIZE + work) {
        report.push(line);
        return None;
    }
    let adf = decode_hfe(label, data, report);
    budget.give_back(work);
    if adf.is_none() {
        budget.give_back(ADF_SIZE);
    }
    adf
}

fn decode_hfe(label: &str, data: &[u8], report: &mut Vec<String>) -> Option<Vec<u8>> {
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
///
/// Past [`INTAKE_BUDGET`] it stops expanding: the input that would pass it and each one after
/// it get a line, PP20 files not yet decrunched stay crunched (one count in the PP20 line), and
/// the result keeps no disks and no files, so nothing starts from part of a set.
pub fn intake(files: Vec<InputFile>) -> Intake {
    let mut report = Vec::new();
    let mut budget = Budget {
        left: INTAKE_BUDGET,
        spent: false,
    };
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
        // An HFE decodes to a whole ADF whatever its size; a dropped ADF is kept as it is.
        let adf = if is_hfe(&f.data) {
            match hfe_to_adf(&f.name, &f.data, &mut budget, &mut report) {
                Some(adf) => adf,
                None => continue,
            }
        } else {
            if let Err(line) = budget.charge(&f.name, f.data.len()) {
                report.push(line);
                continue;
            }
            report.push(format!("{}: ADF {} bytes", f.name, f.data.len()));
            f.data
        };

        let volume = match read_ofs_within(&adf, budget.left) {
            Ok(vol) => {
                let count = vol.files.len();
                // The walk's charge covers the keys and warning lines made from it below; both
                // are built at their exact length, as the walk charged them.
                budget.take_within(vol.held);
                for (path, bytes) in vol.files {
                    let key = joined(&vol.name, "/", &path);
                    let dropped = raw.put(key, bytes, &mut report);
                    budget.give_back(dropped);
                }
                report.push(format!(
                    "{}: volume {} ({}), {count} files",
                    f.name, vol.name, vol.dos_type
                ));
                for w in &vol.warnings {
                    report.push(joined(&f.name, ": ", w));
                }
                if vol.limit_reached {
                    budget.spent = true;
                    report.push(format!(
                        "{}: volume {}: the rest of it would pass the intake's {BUDGET_MIB} MiB \
                         budget and is not read",
                        f.name, vol.name
                    ));
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
        if let Err(line) = budget.charge(&f.name, f.data.len()) {
            report.push(line);
            continue;
        }
        let dropped = raw.put(normalize_path(&f.name), f.data, &mut report);
        budget.give_back(dropped);
    }

    let mut out = BTreeMap::new();
    let (mut packed, mut failed, mut left_crunched) = (0usize, 0usize, 0usize);
    for (path, bytes) in raw.entries {
        if !is_pp20(&bytes) {
            out.insert(path, bytes);
            continue;
        }
        if budget.spent {
            left_crunched += 1;
            out.insert(path, bytes);
            continue;
        }
        // Capped by what is left before the output is allocated.
        match decrunch_pp20_within(&bytes, budget.left) {
            Ok(Some(plain)) => {
                budget.take_within(plain.len());
                budget.give_back(bytes.len());
                out.insert(path, plain);
                packed += 1;
            }
            Ok(None) => {
                budget.spent = true;
                let n = pp20_declared_len(&bytes).unwrap_or(0);
                report.push(format!(
                    "{path}: not decrunched: its {n} bytes would pass the intake's {BUDGET_MIB} \
                     MiB budget"
                ));
                out.insert(path, bytes);
                left_crunched += 1;
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
    let budget_note = if left_crunched > 0 {
        format!(", {left_crunched} left crunched at the budget")
    } else {
        String::new()
    };
    report.push(format!(
        "PP20: decrunched {packed} file(s){failed_note}{budget_note}; {} file(s) total",
        out.len()
    ));

    if budget.spent {
        // The engine reports the files it misses; a part of a set would start and fail later.
        report.push(format!(
            "The intake stopped at its {BUDGET_MIB} MiB budget: no disks or files are kept"
        ));
        return Intake {
            disks: Vec::new(),
            files: BTreeMap::new(),
            report,
        };
    }
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

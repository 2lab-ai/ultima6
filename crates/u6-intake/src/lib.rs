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
/// - each loose file it keeps, and each PP20 output (its crunched bytes given back once done);
/// - each report line while the budget lasts, its text and its slot. A line names its input
///   by a label of at most 258 bytes ([`LABEL_BYTES`] and an ellipsis), so no line passes
///   8,704 bytes (the zip line, which lists 32 labels and counts the rest; any other is under
///   4,400).
///
/// Spent, charged and never given back, so the work an image can cause is bounded too: the bytes
/// a failed file read copied, and a block for each extension table a read walks.
///
/// Transient or outside it, not charged, each bounded apart from it:
/// - the report lines added after the budget is spent: at most [`LINES_AFTER_SPENT`] (32) of
///   at most 8,704 bytes, 278,528 bytes, and three fixed closing lines;
/// - one warning or error string while it is made, before `warn` lists or counts it: a path
///   (at most [`ofs::MAX_PATH_BYTES`], 3,964 bytes) and a few words, reserved at most twice;
/// - the walk's directory paths along its recursion: at most [`ofs::MAX_DEPTH`] of at most
///   3,903 bytes, 249,792 bytes;
/// - the visited set's entries for blocks the walk keeps nothing for (one per block, of an ADF
///   already charged; a kept entry's is in [`ofs::ENTRY_BYTES`]);
/// - an HFE's track table as read before its charge: at most 255 entries of 16 bytes;
/// - the inputs and their names (the caller's), and the zip entries `zip.rs` inflates with their
///   names (at most twice the bytes the zip stores a name in, read as ISO 8859-1), each held
///   until its disk is read; per volume two fixed lines past its [`ofs::MAX_WARNINGS`]
///   warnings.
///
/// The size, from the formats, not from any game file: the game is three Amiga DD disks of
/// [`ADF_SIZE`] (901,120) bytes, 2,703,360 bytes of ADF. A volume's files fit in its blocks, at
/// most 2,703,360 bytes on the three, and PP20 output is at most 56/3 of its crunched stream
/// ([`pp20::MAX_EXPANSION`]), 50,462,720 bytes. While a file decrunches, its crunched bytes
/// (at most one disk, 901,120) are held as well, and the extension tables of 1,757 data blocks
/// cost 25 blocks a disk (38,400 bytes). The entries are at most 1,757 headers and 32 warnings
/// a disk, 5,367 on three; with every name at the root (at most 60 bytes) each is charged
/// 256 + 2 x (61 + 60) = 498 bytes, 2,672,766 in all. The report is at most 38 lines a disk
/// of at most 380 bytes with such names, and the zip line: 47,161 bytes with their slots. That
/// is 56,825,527 bytes (54.2 MiB), and 64 MiB is 18.1% above it (the same 64 MiB the web page
/// allows the game files). Deeper names cost more: a set of three disks fits while its paths
/// average at most 1,018 bytes (about 16 levels of 60-byte names); past that the intake stops
/// at the budget, it does not pass it. The HFE decodes (at most an HFE's size and 290,552 bytes) are given back before
/// any PP20 output.
pub const INTAKE_BUDGET: usize = 64 << 20;

const BUDGET_MIB: usize = INTAKE_BUDGET >> 20;

/// Bytes of an input's name (or a file's path) that a report line or a [`Disk::label`] keeps:
/// a longer one is cut at a character boundary at or below this and ends in an ellipsis, so a
/// label is at most 258 bytes. A zip entry's name can be 65,535 bytes, twice that once read as
/// ISO 8859-1, and its disk is named in up to 38 lines (HFE, missing blocks, volume, 32
/// warnings and the count of the rest, depth, budget).
pub const LABEL_BYTES: usize = 255;
/// Report lines added after the budget is spent; the rest are counted in one line.
pub const LINES_AFTER_SPENT: usize = 32;
/// What a report line costs besides its text: its `String` in the list.
const LINE_SLOT: usize = 24;

/// `name`, or its first [`LABEL_BYTES`] bytes (to a character boundary) and an ellipsis, at its
/// exact length.
fn label(name: &str) -> String {
    if name.len() <= LABEL_BYTES {
        return name.to_string();
    }
    let mut cut = LABEL_BYTES;
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    joined(&name[..cut], "\u{2026}", "")
}

/// What is left of [`INTAKE_BUDGET`], and the report the intake writes against it. The first
/// charge that does not fit spends the budget: every later one fails too, so the intake stops
/// expanding there.
struct Ledger {
    left: usize,
    spent: bool,
    lines: Vec<String>,
    /// Lines added after the budget was spent, and those counted past [`LINES_AFTER_SPENT`].
    after_spent: usize,
    unlisted: usize,
}

impl Ledger {
    /// Charges `n` bytes for the input labelled `name`, or adds its report line when they do not
    /// fit (false).
    fn charge(&mut self, name: &str, n: usize) -> bool {
        if self.spent {
            self.note(format!(
                "{name}: not read: the intake's {BUDGET_MIB} MiB budget is spent"
            ));
            return false;
        }
        if n > self.left {
            self.spent = true;
            self.note(format!(
                "{name}: not read: {n} more bytes would pass the intake's {BUDGET_MIB} MiB budget"
            ));
            return false;
        }
        self.left -= n;
        true
    }

    /// Adds a report line at its exact length, charged with [`LINE_SLOT`] while the budget
    /// lasts; a line that does not fit spends it. After that at most [`LINES_AFTER_SPENT`]
    /// lines are added, uncharged, and the rest counted.
    fn note(&mut self, mut line: String) {
        line.shrink_to_fit();
        if !self.spent {
            let n = line.len() + LINE_SLOT;
            if n <= self.left {
                self.left -= n;
                self.lines.push(line);
                return;
            }
            self.spent = true;
        }
        if self.after_spent < LINES_AFTER_SPENT {
            self.after_spent += 1;
            self.lines.push(line);
        } else {
            self.unlisted += 1;
        }
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
    /// Source name: the dropped file or the zip entry, e.g. "Ultima VI Disk 1.hfe", cut to
    /// [`LABEL_BYTES`] and an ellipsis when longer (the report names the disk the same way).
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
    /// Keeps `bytes` at `path`; a repeated path reports itself and gives back the length of the
    /// earlier copy it drops.
    fn put(&mut self, path: String, bytes: Vec<u8>, ledger: &mut Ledger) {
        match self.index.get(&path) {
            Some(&i) => {
                ledger.note(format!("{}: duplicate path, later copy kept", label(&path)));
                let dropped = std::mem::replace(&mut self.entries[i].1, bytes).len();
                ledger.give_back(dropped);
            }
            None => {
                self.index.insert(path.clone(), self.entries.len());
                self.entries.push((path, bytes));
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
/// The report line lists the first 32 by label and counts the rest.
fn unzip_disks(name: &str, data: &[u8], ledger: &mut Ledger) -> Result<Vec<InputFile>, Error> {
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
    let names: Vec<String> = out.iter().take(32).map(|f| label(&f.name)).collect();
    let mut list = if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    };
    if out.len() > names.len() {
        list.push_str(&format!(", and {} more", out.len() - names.len()));
    }
    ledger.note(format!(
        "{}: zip, {} disk image(s): {list}",
        label(name),
        out.len()
    ));
    Ok(out)
}

/// HFE -> ADF with its report lines, or `None` (reported) when the container is unreadable.
/// Charges the ADF, the track copy and the decode's working bytes before it parses, and gives
/// back all but the ADF once the decode is done.
fn hfe_to_adf(label: &str, data: &[u8], ledger: &mut Ledger) -> Option<Vec<u8>> {
    let work = match hfe_track_bytes(data) {
        Ok(tracks) => tracks + mfm::DECODE_WORK_BYTES,
        Err(err) => {
            ledger.note(format!("{label}: HFE unreadable: {err}"));
            return None;
        }
    };
    if !ledger.charge(label, ADF_SIZE + work) {
        return None;
    }
    let adf = decode_hfe(label, data, ledger);
    ledger.give_back(work);
    if adf.is_none() {
        ledger.give_back(ADF_SIZE);
    }
    adf
}

fn decode_hfe(label: &str, data: &[u8], ledger: &mut Ledger) -> Option<Vec<u8>> {
    let hfe = match parse_hfe(data) {
        Ok(hfe) => hfe,
        Err(err) => {
            ledger.note(format!("{label}: HFE unreadable: {err}"));
            return None;
        }
    };
    let h = &hfe.header;
    let dec = decode_amiga_disk(&hfe);
    ledger.note(format!(
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
        ledger.note(format!(
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
    let mut ledger = Ledger {
        left: INTAKE_BUDGET,
        spent: false,
        lines: Vec::new(),
        after_spent: 0,
        unlisted: 0,
    };
    let mut disk_inputs: Vec<InputFile> = Vec::new();
    let mut loose: Vec<InputFile> = Vec::new();

    for f in files {
        if zip::is_zip(&f.data) {
            match unzip_disks(&f.name, &f.data, &mut ledger) {
                Ok(found) => disk_inputs.extend(found),
                Err(err) => ledger.note(format!(
                    "{}: zip unreadable: {}",
                    label(&f.name),
                    label(&err.to_string())
                )),
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
        // Every line about this disk names it by this, and so does its Disk.
        let name = label(&f.name);
        // An HFE decodes to a whole ADF whatever its size; a dropped ADF is kept as it is.
        let adf = if is_hfe(&f.data) {
            match hfe_to_adf(&name, &f.data, &mut ledger) {
                Some(adf) => adf,
                None => continue,
            }
        } else {
            if !ledger.charge(&name, f.data.len()) {
                continue;
            }
            ledger.note(format!("{name}: ADF {} bytes", f.data.len()));
            f.data
        };

        let volume = match read_ofs_within(&adf, ledger.left) {
            Ok(vol) => {
                let count = vol.files.len();
                // The walk's charge covers the keys and the warnings' text made from it below;
                // the keys are built at their exact length, as the walk charged them.
                ledger.take_within(vol.held);
                for (path, bytes) in vol.files {
                    raw.put(joined(&vol.name, "/", &path), bytes, &mut ledger);
                }
                ledger.note(format!(
                    "{name}: volume {} ({}), {count} files",
                    vol.name, vol.dos_type
                ));
                for w in &vol.warnings {
                    ledger.note(joined(&name, ": ", w));
                }
                if vol.limit_reached {
                    ledger.spent = true;
                    ledger.note(format!(
                        "{name}: volume {}: the rest of it would pass the intake's {BUDGET_MIB} \
                         MiB budget and is not read",
                        vol.name
                    ));
                }
                Some(vol.name)
            }
            Err(err) => {
                ledger.note(format!("{name}: filesystem unreadable: {err}"));
                None
            }
        };
        disks.push(Disk {
            label: name,
            volume,
            adf,
        });
    }

    for f in loose {
        if !ledger.charge(&label(&f.name), f.data.len()) {
            continue;
        }
        raw.put(normalize_path(&f.name), f.data, &mut ledger);
    }

    let mut out = BTreeMap::new();
    let (mut packed, mut failed, mut left_crunched) = (0usize, 0usize, 0usize);
    for (path, bytes) in raw.entries {
        if !is_pp20(&bytes) {
            out.insert(path, bytes);
            continue;
        }
        if ledger.spent {
            left_crunched += 1;
            out.insert(path, bytes);
            continue;
        }
        // Capped by what is left before the output is allocated.
        match decrunch_pp20_within(&bytes, ledger.left) {
            Ok(Some(plain)) => {
                ledger.take_within(plain.len());
                ledger.give_back(bytes.len());
                out.insert(path, plain);
                packed += 1;
            }
            Ok(None) => {
                ledger.spent = true;
                let n = pp20_declared_len(&bytes).unwrap_or(0);
                ledger.note(format!(
                    "{}: not decrunched: its {n} bytes would pass the intake's {BUDGET_MIB} MiB \
                     budget",
                    label(&path)
                ));
                out.insert(path, bytes);
                left_crunched += 1;
            }
            Err(err) => {
                ledger.note(format!(
                    "{}: PP20 decrunch failed, kept crunched: {err}",
                    label(&path)
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
    let mut report = ledger.lines;
    // The closing lines, fixed and uncharged.
    if ledger.unlisted > 0 {
        report.push(format!(
            "{} more report lines not listed: the intake's {BUDGET_MIB} MiB budget is spent",
            ledger.unlisted
        ));
    }
    report.push(format!(
        "PP20: decrunched {packed} file(s){failed_note}{budget_note}; {} file(s) total",
        out.len()
    ));

    if ledger.spent {
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

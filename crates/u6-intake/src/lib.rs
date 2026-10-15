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

/// The bytes the intake may hold in all (64 MiB). Every byte it keeps is in one of the lists
/// below: charged, or outside the budget with its own bound.
///
/// Retained, charged before it is allocated and given back when the intake drops it. Strings
/// are built at their exact length and a list's growth is charged as it is made (old and new
/// buffers while it moves), so a charge is what the allocation holds:
/// - the [`FIRST_ALLOCATIONS`] of its lists and maps, once; the lists the inputs are sorted
///   into (48-byte slots) and, while the disk images are sorted, the sort's scratch (one slot
///   each);
/// - each ADF it keeps: a dropped one's buffer as it came (spare capacity included), an HFE's
///   decoded one; while an HFE decodes, also
///   the tracks the parser copies out ([`hfe_track_bytes`]) and the decode's working bytes
///   ([`mfm::DECODE_WORK_BYTES`]: one track's cells, its sectors, the missing list); with it
///   the disk's label (at most 258 bytes) and the disk list's 72-byte slots;
/// - each volume's name, its files as reserved, their paths (twice, as key and index, with
///   room for the volume name), the directory paths and the listed warnings, each with
///   [`ofs::ENTRY_BYTES`] for its slots in lists and maps ([`OfsVolume::held`]);
/// - each loose file it keeps: its buffer as it came (spare capacity included), its path twice
///   and [`ofs::ENTRY_BYTES`], as a volume's file; and each PP20 output (the crunched buffer
///   given back once done);
/// - each report line while the budget lasts, and the list's slots, which always keep room
///   for the 35 lines that may follow the spend. A line names its input by a label of at most
///   258 bytes ([`LABEL_BYTES`] and an ellipsis), so no line passes 8,704 bytes (the zip line,
///   which lists 32 labels and counts the rest; any other is under 4,400).
///
/// Spent, charged and never given back, so the work an image can cause is bounded too: the bytes
/// a failed file read copied, and a block for each extension table a read walks.
///
/// Outside the budget, each bounded apart from it:
/// - the text of the report lines added after the budget is spent: at most
///   [`LINES_AFTER_SPENT`] (32) of at most 8,704 bytes, 278,528 bytes, and three closing lines
///   of under 100 bytes (their slots are charged);
/// - one string while it is made, before it is cut to its length: a report line, a warning or
///   an error, at most twice 8,704 bytes;
/// - a loose file's path while it is made and charged: twice its name, one at a time;
/// - two names compared while the disk images are sorted: a copy of each one's digit runs, at
///   most both names' lengths, one comparison at a time;
/// - the walk's directory paths along its recursion: at most [`ofs::MAX_DEPTH`] of at most
///   3,903 bytes, 249,792 bytes;
/// - the visited set's entries for blocks the walk keeps nothing for: one per block of an ADF
///   already charged, at most 28 bytes each (a B-tree leaf of 104 bytes holds at least 5
///   keys, with its share of the inner nodes), under 5.5% of that ADF;
/// - an HFE's track table as read before its charge: at most 255 entries of 16 bytes;
/// - the inputs until the intake takes them over, and their names: the caller's, each freed once
///   read (an HFE's buffer too: only its decoded ADF is kept);
/// - the zip entries `zip.rs` extracts and their names, each held until its disk is read:
///   bounded by `zip.rs` (at most `MAX_ENTRIES`, 65,534, entries and `MAX_DIRECTORY_BYTES`,
///   16 MiB, of directory, so at most 32 MiB of names read as ISO 8859-1) and by the web page's
///   64 MiB cap on what the disk entries unpack to.
///
/// The size, from the formats, not from any game file: the game is three Amiga DD disks of
/// [`ADF_SIZE`] (901,120) bytes, 2,703,360 bytes of ADF. A volume's files fit in its blocks, at
/// most 2,703,360 bytes on the three, and PP20 output is at most 56/3 of its crunched stream
/// ([`pp20::MAX_EXPANSION`]), 50,462,720 bytes. While a file decrunches, its crunched bytes
/// (at most one disk, 901,120) are held as well, and the extension tables of 1,757 data blocks
/// cost 25 blocks a disk (38,400 bytes). The entries are at most 1,757 headers and 32 warnings
/// a disk, 5,367 on three; with every name at the root (at most 60 bytes) each is charged
/// 384 + 2 x (61 + 60) = 626 bytes, 3,359,742 in all. The report is at most 38 lines a disk
/// of at most 380 bytes with such names and the zip line, with its list: 55,345 bytes; the
/// disks' labels, slots and volume names 1,530; the first allocations, the inputs' list and
/// the sort's scratch 4,624. That is 57,526,841 bytes (54.9 MiB), and 64 MiB is 16.7% above it
/// (the same 64 MiB the web page allows the game files). Deeper names cost more: a set of
/// three disks fits while its paths average at most 952 bytes (about 15 levels of 60-byte
/// names); past that the intake stops at the budget, it does not pass it.
/// The HFE decodes (at most an HFE's size and 290,552 bytes) are given back before any PP20
/// output.
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
/// Room the report's list always keeps, charged: the lines after the budget is spent and the
/// three closing lines, so adding them never grows it.
const TAIL: usize = LINES_AFTER_SPENT + 3;
/// The first allocations of the lists and maps that [`ofs::ENTRY_BYTES`] does not cover while
/// they hold few entries, charged once: the collected files' list (4 slots of 48 bytes) and
/// index (a 368-byte root leaf), the result's map (a 540-byte root leaf), and the walk's
/// lists and maps for the volume being read (files 192, index 368, visited set 100, directories
/// and warnings 96 each): 1,952 bytes, 4,096 with room to spare.
const FIRST_ALLOCATIONS: usize = 4096;

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
    /// The least `left` has been: the most the intake had charged at once.
    low: usize,
}

impl Ledger {
    /// The whole budget, less the report list's first [`TAIL`] slots and the
    /// [`FIRST_ALLOCATIONS`].
    fn new(budget: usize) -> Ledger {
        let left = budget - TAIL * size_of::<String>() - FIRST_ALLOCATIONS;
        Ledger {
            left,
            spent: false,
            lines: Vec::with_capacity(TAIL),
            after_spent: 0,
            unlisted: 0,
            low: left,
        }
    }

    /// Takes `n` bytes the caller has checked fit.
    fn take(&mut self, n: usize) {
        self.left = self.left.saturating_sub(n);
        self.low = self.low.min(self.left);
    }

    /// Makes room for `v` to take over one more input, as [`Ledger::room`]; when it does not
    /// fit, the input is refused in the report instead.
    fn stage(&mut self, v: &mut Vec<InputFile>, f: InputFile) {
        if self.room(v, 0) {
            v.push(f);
        } else {
            self.charge(&label(&f.name), 0);
        }
    }

    /// Makes room for one more element in `v`, charging the growth before it is allocated:
    /// twice the capacity (at least `min`, and 8), with the old buffer counted until the move is
    /// done. False (nothing allocated, the budget spent) when it does not fit.
    fn room<T>(&mut self, v: &mut Vec<T>, min: usize) -> bool {
        let (len, cap) = (v.len(), v.capacity());
        if cap >= min.max(len + 1) {
            return true;
        }
        let want = (cap * 2).max(min).max(len + 1).max(8);
        let need = want * size_of::<T>();
        if self.spent || need > self.left {
            self.spent = true;
            return false;
        }
        self.take(need);
        v.reserve_exact(want - len);
        self.give_back(cap * size_of::<T>());
        // The allocator may hand out more than was asked: that is held too.
        let extra = v.capacity().saturating_sub(want) * size_of::<T>();
        self.take(extra);
        true
    }

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
        self.take(n);
        true
    }

    /// Adds a report line at its exact length, charged (with any growth of the list) while the
    /// budget lasts; a line that does not fit spends it. After that at most
    /// [`LINES_AFTER_SPENT`] lines are added, uncharged, in the room the list keeps, and the
    /// rest counted.
    fn note(&mut self, mut line: String) {
        line.shrink_to_fit();
        if !self.spent && line.len() <= self.left {
            self.take(line.len());
            let mut lines = std::mem::take(&mut self.lines);
            let min = lines.len() + 1 + TAIL;
            let fits = self.room(&mut lines, min);
            self.lines = lines;
            if fits {
                self.lines.push(line);
                return;
            }
            self.give_back(line.len());
        }
        self.spent = true;
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
        self.take(n);
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
    /// Keeps `bytes` at `path`, which was charged `key` for its two copies and slots; a
    /// repeated path reports itself and gives back the earlier copy it drops and the `key` it
    /// does not keep again.
    fn put(&mut self, path: String, bytes: Vec<u8>, key: usize, ledger: &mut Ledger) {
        match self.index.get(&path) {
            Some(&i) => {
                // The earlier copy was charged its capacity.
                let dropped = std::mem::replace(&mut self.entries[i].1, bytes).capacity();
                ledger.give_back(dropped + key);
                ledger.note(format!("{}: duplicate path, later copy kept", label(&path)));
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
    // The disk keeps its label: charged with the ADF.
    let kept = ADF_SIZE + label.len();
    if !ledger.charge(label, kept + work) {
        return None;
    }
    let adf = decode_hfe(label, data, ledger);
    ledger.give_back(work);
    if adf.is_none() {
        ledger.give_back(kept);
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
    intake_within(files, INTAKE_BUDGET).0
}

/// [`intake`] against `budget`, with the most it charged at once.
fn intake_within(files: Vec<InputFile>, budget: usize) -> (Intake, usize) {
    let mut ledger = Ledger::new(budget);
    // The inputs move into these lists, whose slots are charged as they grow.
    let mut disk_inputs: Vec<InputFile> = Vec::new();
    let mut loose: Vec<InputFile> = Vec::new();

    for f in files {
        if zip::is_zip(&f.data) {
            match unzip_disks(&f.name, &f.data, &mut ledger) {
                Ok(found) => {
                    for entry in found {
                        ledger.stage(&mut disk_inputs, entry);
                    }
                }
                Err(err) => ledger.note(format!(
                    "{}: zip unreadable: {}",
                    label(&f.name),
                    label(&err.to_string())
                )),
            }
        } else if is_hfe(&f.data) || is_adf(&f.name, &f.data) {
            ledger.stage(&mut disk_inputs, f);
        } else {
            ledger.stage(&mut loose, f);
        }
    }
    // The sort's scratch holds at most one slot per disk input while it runs.
    let scratch = disk_inputs.len() * size_of::<InputFile>();
    if !ledger.spent && scratch <= ledger.left {
        ledger.take(scratch);
        disk_inputs.sort_by(|a, b| by_label(&a.name, &b.name));
        ledger.give_back(scratch);
    } else {
        ledger.spent = true;
    }
    let staged = |v: &Vec<InputFile>| v.capacity() * size_of::<InputFile>();
    let (disk_slots, loose_slots) = (staged(&disk_inputs), staged(&loose));

    let mut disks = Vec::new();
    let mut raw = Collected::default();
    for f in disk_inputs {
        // Every line about this disk names it by this, and so does its Disk, which keeps it:
        // charged with the ADF.
        let name = label(&f.name);
        // An HFE decodes to a whole ADF whatever its size; a dropped ADF is kept as it is.
        let adf = if is_hfe(&f.data) {
            match hfe_to_adf(&name, &f.data, &mut ledger) {
                Some(adf) => adf,
                None => continue,
            }
        } else {
            // Its buffer as it came, spare capacity included.
            if !ledger.charge(&name, f.data.capacity() + name.capacity()) {
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
                    let key = ofs::key_charge(path.len());
                    raw.put(joined(&vol.name, "/", &path), bytes, key, &mut ledger);
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
        // Once the budget is spent nothing is kept: the disk is not added (nor its slot grown).
        if ledger.room(&mut disks, 0) {
            disks.push(Disk {
                label: name,
                volume,
                adf,
            });
        }
    }

    ledger.give_back(disk_slots);

    for f in loose {
        // Its buffer as it came (spare capacity included), and its path kept twice (key and
        // index) with its slots, as a volume's file.
        let path = normalize_path(&f.name);
        let key = 2 * path.capacity() + ofs::ENTRY_BYTES;
        if !ledger.charge(&label(&f.name), f.data.capacity() + key) {
            continue;
        }
        raw.put(path, f.data, key, &mut ledger);
    }
    ledger.give_back(loose_slots);

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
                // Both as held: the output exactly its length, the crunched file as charged.
                ledger.take_within(plain.capacity());
                ledger.give_back(bytes.capacity());
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
        let empty = Intake {
            disks: Vec::new(),
            files: BTreeMap::new(),
            report,
        };
        return (empty, budget - ledger.low);
    }
    let kept = Intake {
        disks,
        files: out,
        report,
    };
    (kept, budget - ledger.low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    /// Counts the bytes allocated on each thread, as tests/budget.rs does.
    struct Counting;

    thread_local! {
        static LIVE: Cell<isize> = const { Cell::new(0) };
        static PEAK: Cell<isize> = const { Cell::new(0) };
    }

    fn count(delta: isize) {
        let _ = LIVE.try_with(|live| {
            let now = live.get() + delta;
            live.set(now);
            let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
        });
    }

    // SAFETY: every call is forwarded to the system allocator unchanged; only counters change.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc(layout) };
            if !p.is_null() {
                count(layout.size() as isize);
            }
            p
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc_zeroed(layout) };
            if !p.is_null() {
                count(layout.size() as isize);
            }
            p
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) };
            count(-(layout.size() as isize));
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let p = unsafe { System.realloc(ptr, layout, new_size) };
            if !p.is_null() {
                count(new_size as isize);
                count(-(layout.size() as isize));
            }
            p
        }
    }

    #[global_allocator]
    static ALLOC: Counting = Counting;

    /// The most `intake_within` held at once beyond its inputs, and the most it had charged
    /// beyond the inputs' buffers it keeps (charged, but not allocated by it).
    fn held_and_charged(files: Vec<InputFile>) -> (usize, usize) {
        let moved: usize = files.iter().map(|f| f.data.capacity()).sum();
        let start = LIVE.with(Cell::get);
        PEAK.with(|p| p.set(start));
        let (_, charged) = intake_within(files, INTAKE_BUDGET);
        let peak = PEAK.with(Cell::get) - start;
        (peak.max(0) as usize, charged - moved)
    }

    /// An ADF whose root holds `n` empty files (no checksums set: each block warns).
    fn adf_with_files(n: usize) -> Vec<u8> {
        let mut img = vec![0u8; ADF_SIZE];
        img[..4].copy_from_slice(b"DOS\0");
        let w = |img: &mut [u8], b: usize, off: usize, v: u32| {
            img[b * 512 + off..b * 512 + off + 4].copy_from_slice(&v.to_be_bytes());
        };
        w(&mut img, 880, 0, 2);
        w(&mut img, 880, 12, 72);
        w(&mut img, 880, 508, 1);
        img[880 * 512 + 432] = 1;
        img[880 * 512 + 433] = b'V';
        for k in 0..n {
            let b = 882 + k;
            w(&mut img, b, 0, 2);
            w(&mut img, b, 4, b as u32);
            w(&mut img, b, 508, (-3i32) as u32);
            img[b * 512 + 432] = 2;
            img[b * 512 + 433] = b'f';
            img[b * 512 + 434] = b'a' + k as u8;
            w(&mut img, 880, 24 + k * 4, b as u32);
        }
        img
    }

    #[test]
    fn few_entries_are_charged_for_the_first_allocations() {
        // With nothing or a handful of entries, the lists' and maps' first allocations (root
        // nodes, first capacities) are most of what the intake holds.
        let loose = |n: usize| -> Vec<InputFile> {
            (0..n)
                .map(|i| InputFile {
                    name: format!("f{i}"),
                    data: vec![i as u8; 10],
                })
                .collect()
        };
        let disk = |n: usize| {
            vec![InputFile {
                name: "d.adf".into(),
                data: adf_with_files(n),
            }]
        };
        let cases: Vec<(String, Vec<InputFile>)> = vec![
            ("nothing".into(), Vec::new()),
            ("1 loose".into(), loose(1)),
            ("5 loose".into(), loose(5)),
            ("1 file on a disk".into(), disk(1)),
            ("2 files on a disk".into(), disk(2)),
            ("5 files on a disk".into(), disk(5)),
        ];
        for (what, files) in cases {
            let (held, charged) = held_and_charged(files);
            assert!(held <= charged, "{what}: held {held}, charged {charged}");
        }
    }

    #[test]
    fn the_report_is_charged_for_the_list_that_holds_it() {
        // Short lines until they spend a 1 MiB budget, then 100 more: what the report holds,
        // the list's capacity included, is within the budget but for the lines kept after it.
        let budget = 1 << 20;
        let mut l = Ledger::new(budget);
        let mut n = 0;
        while !l.spent {
            l.note(format!("{n}"));
            n += 1;
        }
        for k in 0..100 {
            l.note(format!("after {k}"));
        }
        assert_eq!((l.after_spent, l.unlisted), (32, 100 - 31));
        let after: usize = l.lines[l.lines.len() - 32..]
            .iter()
            .map(String::capacity)
            .sum();
        let held = l.lines.capacity() * size_of::<String>()
            + l.lines.iter().map(String::capacity).sum::<usize>();
        assert!(
            held - after <= budget,
            "the report holds {} bytes for a budget of {budget}",
            held - after
        );
        // The closing lines fit without growing the list.
        let cap = l.lines.capacity();
        for _ in 0..3 {
            l.lines.push(String::new());
        }
        assert_eq!(l.lines.capacity(), cap);
    }

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

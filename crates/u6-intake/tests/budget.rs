//! The intake's total output is bounded: synthetic inputs that would make it produce far more
//! than they hold (PP20 files, disk images, volumes whose files share blocks, loose files) stop
//! at the budget. A counting allocator measures what the intake allocates on the test's thread.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::ofs_builder::{
    ImageBuilder, Node, deep_latin1_files, empty_table_loops, nested_dirs, shared_block_volume,
    shared_short_chain,
};
use common::pp20_encoder::max_ratio_pp20;
use u6_intake::{
    InputFile, Intake, decrunch_pp20_within, intake, pp20_declared_len, read_ofs, read_ofs_within,
};

/// The budget these tests hold the intake to: 64 MiB (the derivation is at `INTAKE_BUDGET` in
/// the crate). Stated here, not imported, so the tests run against any version of the intake.
const BOUND: usize = 64 << 20;
const MIB: usize = 1 << 20;

/// Counts the bytes allocated on each thread (tests run on their own threads). A reallocation
/// counts the new block before the old one is freed, as the allocator may hold both.
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

/// Runs `f` and returns its result with the most bytes it held allocated at once, beyond what
/// was allocated before it ran (the inputs it is given).
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let out = f();
    let peak = PEAK.with(Cell::get) - start;
    (out, peak.max(0) as usize)
}

fn file(name: String, data: Vec<u8>) -> InputFile {
    InputFile { name, data }
}

fn output_bytes(r: &Intake) -> usize {
    r.disks.iter().map(|d| d.adf.len()).sum::<usize>()
        + r.files.values().map(Vec::len).sum::<usize>()
}

/// The report lines that start with `name: ` (one input's lines).
fn lines_of<'a>(r: &'a Intake, name: &str) -> Vec<&'a str> {
    let head = format!("{name}: ");
    r.report
        .iter()
        .filter(|l| l.starts_with(&head))
        .map(String::as_str)
        .collect()
}

const STOP_LINE: &str = "The intake stopped at its 64 MiB budget: no disks or files are kept";

#[test]
fn pp20_files_that_really_expand_to_16_mib_stop_at_the_budget() {
    // Six small files (under 1 MiB each) whose length fields claim 16 MiB and whose streams
    // really produce it: 96 MiB in all.
    let crunched = max_ratio_pp20((1 << 24) - 1, 0x5a);
    assert!(crunched.len() < MIB);
    let inputs: Vec<InputFile> = (0..6)
        .map(|i| file(format!("p{i}"), crunched.clone()))
        .collect();
    drop(crunched);
    let (r, peak) = peak_of(|| intake(inputs));
    assert!(
        peak <= BOUND,
        "the intake held {peak} bytes at once, over {BOUND}"
    );
    let out = output_bytes(&r);
    assert!(out <= BOUND, "the intake kept {out} bytes, over {BOUND}");
    // Three fit (with the six crunched files they come to about 53 MiB); the fourth is refused
    // before its output is allocated, and nothing is kept.
    assert_eq!(
        lines_of(&r, "p3"),
        ["p3: not decrunched: its 16777215 bytes would pass the intake's 64 MiB budget"],
        "{:#?}",
        r.report
    );
    for i in [0, 1, 2, 4, 5] {
        assert!(lines_of(&r, &format!("p{i}")).is_empty(), "{:#?}", r.report);
    }
    assert!(
        r.report
            .iter()
            .any(|l| l
                == "PP20: decrunched 3 file(s), 3 left crunched at the budget; 6 file(s) total"),
        "{:#?}",
        r.report
    );
    assert_eq!(r.report.last().map(String::as_str), Some(STOP_LINE));
    assert!(r.files.is_empty() && r.disks.is_empty());
}

#[test]
fn tiny_pp20_files_claiming_16_mib_allocate_nothing_for_it() {
    // 16 bytes each: magic, offset widths, four stream bytes, then a 16 MiB length field.
    let tiny = [
        b"PP20".as_slice(),
        &[9, 10, 11, 11],
        &[0xa5; 4],
        &[0xff, 0xff, 0xff, 0],
    ]
    .concat();
    let inputs: Vec<InputFile> = (0..32)
        .map(|i| file(format!("t{i}"), tiny.clone()))
        .collect();
    let (r, peak) = peak_of(|| intake(inputs));
    assert!(
        peak < MIB,
        "the intake held {peak} bytes at once for 512 bytes of input"
    );
    // Each is kept as it came, its claim refused as more than its stream can hold.
    assert_eq!(r.files.len(), 32);
    assert!(r.files.values().all(|v| *v == tiny));
    for i in 0..32 {
        let name = format!("t{i}");
        assert_eq!(
            lines_of(&r, &name),
            [format!(
                "{name}: PP20 decrunch failed, kept crunched: PP20: declares 16777215 bytes, \
                 more than its stream can hold"
            )],
            "{:#?}",
            r.report
        );
    }
    assert_eq!(
        r.report.last().map(String::as_str),
        Some("PP20: decrunched 0 file(s), 32 failed; 32 file(s) total")
    );
}

#[test]
fn a_loose_file_set_over_the_budget_stops_there() {
    // 66 plain files of 1 MiB: the first 64 fill the budget exactly.
    let inputs: Vec<InputFile> = (0..66)
        .map(|i| file(format!("loose/f{i:02}"), vec![i as u8; MIB]))
        .collect();
    let r = intake(inputs);
    let out = output_bytes(&r);
    assert!(out <= BOUND, "the intake kept {out} bytes, over {BOUND}");
    assert_eq!(
        lines_of(&r, "loose/f64"),
        ["loose/f64: not read: 1048576 more bytes would pass the intake's 64 MiB budget"]
    );
    assert_eq!(
        lines_of(&r, "loose/f65"),
        ["loose/f65: not read: the intake's 64 MiB budget is spent"]
    );
    for i in 0..64 {
        assert!(lines_of(&r, &format!("loose/f{i:02}")).is_empty());
    }
    assert_eq!(r.report.last().map(String::as_str), Some(STOP_LINE));
    assert!(r.files.is_empty());
}

/// An HFE with no cylinders: 512 bytes that decode to a whole (blank) 901,120-byte ADF.
fn empty_hfe() -> Vec<u8> {
    let mut h = vec![0u8; 512];
    h[..8].copy_from_slice(b"HXCPICFE");
    h[10] = 1; // one side
    h[12] = 250; // kbps
    h[18] = 1; // the track table at block 1 (no entries)
    h
}

/// An HFE of `cylinders` cylinders, two sides, each naming `length` bytes of (blank) track data
/// in its own run of blocks, which the parser copies out whole before decoding.
fn tracked_hfe(cylinders: usize, length: u16) -> Vec<u8> {
    let blocks = usize::from(length).div_ceil(512);
    let first = 1 + (cylinders * 4).div_ceil(512); // after the header and the track table
    let mut h = vec![0u8; (first + cylinders * blocks) * 512];
    h[..8].copy_from_slice(b"HXCPICFE");
    h[9] = cylinders as u8;
    h[10] = 2;
    h[11] = 1;
    h[12] = 250;
    h[18] = 1;
    for cyl in 0..cylinders {
        let at = 512 + cyl * 4;
        h[at..at + 2].copy_from_slice(&((first + cyl * blocks) as u16).to_le_bytes());
        h[at + 2..at + 4].copy_from_slice(&length.to_le_bytes());
    }
    h
}

#[test]
fn disk_images_past_the_budget_are_not_decoded() {
    // 80 cylinders of 1,024 bytes: each disk holds its ADF, 81,920 bytes of tracks and the
    // decode's 290,552 working bytes while it decodes, and keeps its 901,120-byte ADF.
    // 76 disks would keep 68,485,120 bytes; 74 fit.
    let hfe = tracked_hfe(80, 1024);
    let inputs: Vec<InputFile> = (0..76)
        .map(|i| file(format!("disk {i}.hfe"), hfe.clone()))
        .collect();
    drop(hfe);
    let (r, peak) = peak_of(|| intake(inputs));
    assert!(
        peak <= BOUND,
        "the intake held {peak} bytes at once, over {BOUND}"
    );
    let out = output_bytes(&r);
    assert!(out <= BOUND, "the intake kept {out} bytes, over {BOUND}");
    assert_eq!(
        lines_of(&r, "disk 74.hfe"),
        ["disk 74.hfe: not read: 1273592 more bytes would pass the intake's 64 MiB budget"]
    );
    assert_eq!(
        lines_of(&r, "disk 75.hfe"),
        ["disk 75.hfe: not read: the intake's 64 MiB budget is spent"]
    );
    assert!(
        lines_of(&r, "disk 73.hfe")
            .iter()
            .all(|l| !l.contains("budget"))
    );
    assert_eq!(r.report.last().map(String::as_str), Some(STOP_LINE));
    assert!(r.disks.is_empty() && r.files.is_empty());
}

#[test]
fn an_hfe_whose_track_copy_passes_the_budget_is_not_decoded() {
    // 73 blank disks keep 65,781,760 bytes. The 74th disk's ADF would fit after them; the
    // 16,711,170 bytes of tracks its decode copies out first (255 cylinders of 65,534 bytes,
    // the most an HFE can name without overlap) would not.
    let mut inputs: Vec<InputFile> = (0..73)
        .map(|i| file(format!("disk {i}.hfe"), empty_hfe()))
        .collect();
    inputs.push(file("disk 99.hfe".into(), tracked_hfe(255, 65_534)));
    let (r, peak) = peak_of(|| intake(inputs));
    assert!(
        peak <= BOUND,
        "the intake held {peak} bytes at once, over {BOUND}"
    );
    assert_eq!(
        lines_of(&r, "disk 99.hfe"),
        ["disk 99.hfe: not read: 17902842 more bytes would pass the intake's 64 MiB budget"]
    );
    assert_eq!(r.report.last().map(String::as_str), Some(STOP_LINE));
    assert!(r.disks.is_empty() && r.files.is_empty());
}

#[test]
fn deep_directories_stop_at_the_depth_cap() {
    // 1,757 nested directories with 30-character names (every block of the disk): their paths
    // alone come to 31 x 1,757^2 / 2, about 48 MB.
    let adf = nested_dirs("DEEP", 1757, false);
    let (r, peak) = peak_of(|| intake(vec![file("deep.adf".into(), adf)]));
    assert!(peak < 2 * MIB, "the intake held {peak} bytes at once");
    assert_eq!(
        lines_of(&r, "deep.adf"),
        [
            "deep.adf: ADF 901120 bytes",
            "deep.adf: volume DEEP (DOS0), 0 files",
            "deep.adf: directories nested more than 64 levels deep are not read",
        ]
    );
    assert_eq!(r.disks.len(), 1);
    assert_eq!(read_ofs(&r.disks[0].adf).unwrap().dirs.len(), 64);
}

#[test]
fn loop_warnings_are_capped_per_volume() {
    // 200 levels whose other 71 hash slots each name a block the walk has seen: 71 warnings
    // on every level, each carrying the level's whole path.
    let adf = nested_dirs("LOOPS", 200, true);
    let (r, peak) = peak_of(|| intake(vec![file("loops.adf".into(), adf)]));
    assert!(peak < 2 * MIB, "the intake held {peak} bytes at once");
    let lines = lines_of(&r, "loops.adf");
    // The ADF and volume lines, 32 warnings, the depth line, then the count of the rest: 64
    // levels are read, 71 warnings each. The deepest level read comes first (slot 0 holds the
    // level below, walked before the other slots), at block 65.
    assert_eq!(lines.len(), 36, "{:#?}", &lines[..lines.len().min(40)]);
    let deepest: Vec<String> = (0..64).map(|k| format!("level{k:025}")).collect();
    assert_eq!(
        lines[2],
        format!("loops.adf: {}: loop at block 65", deepest.join("/"))
    );
    assert_eq!(
        lines[34],
        "loops.adf: directories nested more than 64 levels deep are not read"
    );
    assert_eq!(lines[35], "loops.adf: 4512 more warnings not listed");
    assert_eq!(r.disks.len(), 1);
}

#[test]
fn a_limit_refuses_a_longer_claim_before_allocating() {
    let len = 4 << 20;
    let packed = max_ratio_pp20(len, 7);
    assert_eq!(pp20_declared_len(&packed), Some(len));
    let (refused, peak) = peak_of(|| decrunch_pp20_within(&packed, len - 1));
    assert!(matches!(refused, Ok(None)));
    assert!(peak < 64 << 10, "a refused claim held {peak} bytes");
    // The same call within the limit allocates the output (the counter sees it).
    let (done, peak) = peak_of(|| decrunch_pp20_within(&packed, len));
    assert!(matches!(&done, Ok(Some(v)) if v.len() == len && v.iter().all(|&b| b == 7)));
    assert!(peak >= len, "{peak}");
    // A bad file is an error under any limit, and the capacity check comes first.
    assert!(decrunch_pp20_within(b"PP20", 0).is_err());
    assert_eq!(pp20_declared_len(b"PP20"), None);
    let tiny = [b"PP20".as_slice(), &[0; 4], &[0; 4], &[0xff, 0xff, 0xff, 0]].concat();
    assert!(decrunch_pp20_within(&tiny, 0).is_err());
}

#[test]
fn volume_files_that_share_blocks_stop_at_the_budget() {
    // 140 files of 512 KiB out of one 880 KiB image: 70 MiB.
    let adf = shared_block_volume(140, 512 << 10);
    let (r, peak) = peak_of(|| intake(vec![file("shared.adf".into(), adf)]));
    assert!(
        peak <= BOUND,
        "the intake held {peak} bytes at once, over {BOUND}"
    );
    let out = output_bytes(&r);
    assert!(out <= BOUND, "the intake kept {out} bytes, over {BOUND}");
    let lines = lines_of(&r, "shared.adf");
    assert!(
        lines.contains(
            &"shared.adf: volume SHARED: the rest of it would pass the intake's 64 MiB budget \
              and is not read"
        ),
        "{lines:#?}"
    );
    assert_eq!(r.report.last().map(String::as_str), Some(STOP_LINE));
    assert!(r.disks.is_empty() && r.files.is_empty());
}

#[test]
fn a_file_header_claiming_more_than_its_image_allocates_nothing_for_it() {
    // One header claiming 64 MiB that loops on itself as its own extension block.
    let adf = shared_block_volume(1, 64 << 20);
    let (vol, peak) = peak_of(|| read_ofs(&adf).unwrap());
    assert!(
        peak < 2 * MIB,
        "reading the volume held {peak} bytes at once"
    );
    assert!(vol.file("s000").is_none());
    assert!(
        vol.warnings
            .iter()
            .any(|w| w == "s000: byte size 67108864 passes the image size 901120"),
        "{:?}",
        vol.warnings
    );
}

#[test]
fn many_small_files_are_charged_for_their_bookkeeping() {
    // 877 empty files with 30-character names (every block the builder can give them): their
    // bytes are nothing, their list and map entries are not.
    let names: Vec<String> = (0..877).map(|i| format!("f{i:029}")).collect();
    let nodes: Vec<Node> = names
        .iter()
        .map(|n| Node::File(Box::leak(n.clone().into_boxed_str()), Vec::new()))
        .collect();
    let adf = ImageBuilder::new(false).build("BOOK", &nodes);
    let (vol, walk) = peak_of(|| read_ofs(&adf).unwrap());
    assert_eq!(vol.files.len(), 877);
    assert!(
        walk <= vol.held,
        "the walk held {walk} bytes, charged {}",
        vol.held
    );
    let held = vol.held;
    drop(vol);
    let (r, whole) = peak_of(|| intake(vec![file("book.adf".into(), adf)]));
    assert_eq!(r.files.len(), 877);
    assert!(
        whole <= held,
        "the intake held {whole} bytes, charged {held}"
    );
}

#[test]
fn deep_paths_in_two_byte_characters_are_charged_as_held() {
    // 6,900 empty files 63 directories down, every name 30 characters of two bytes each: each
    // path is 3,903 bytes, so the paths come to 27 MB, and each is held more than once.
    let adf = deep_latin1_files(7000, 63, 6900);
    let (r, peak) = peak_of(|| intake(vec![file("deep.adf".into(), adf)]));
    assert!(
        peak <= BOUND,
        "the intake held {peak} bytes at once, over {BOUND}"
    );
    // They fit: the walk charges what the intake holds, it does not refuse them.
    assert_eq!(
        r.files.len(),
        6900,
        "{:#?}",
        &r.report[..r.report.len().min(6)]
    );
    assert!(r.files.keys().all(|k| k.len() == "DEEP/".len() + 3903));
}

/// The number of reads that failed with `what`, listed or counted in the last warning.
fn failed_reads(warnings: &[String], what: &str) -> usize {
    let listed = warnings.iter().filter(|w| w.contains(what)).count();
    let unlisted = warnings
        .last()
        .and_then(|w| w.strip_suffix(" more warnings not listed"))
        .map_or(0, |n| n.parse::<usize>().unwrap());
    listed + unlisted
}

#[test]
fn reads_that_fail_on_a_shared_chain_copy_at_most_the_limit() {
    // 200 headers share one extension chain of 884,736 bytes and each claims a byte more:
    // every read copies the chain and fails. Under an 8 MiB limit the copying stops at it.
    let limit = 8 << 20;
    let img = shared_short_chain(200);
    let vol = read_ofs_within(&img, limit).unwrap();
    let reads = failed_reads(&vol.warnings, "short read 884736/884737");
    assert!(vol.limit_reached, "{:?}", vol.warnings);
    assert!(
        reads * 884_736 <= limit,
        "{reads} reads copied {} bytes under a limit of {limit}",
        reads * 884_736
    );
}

#[test]
fn reads_that_walk_empty_tables_stop_at_the_limit() {
    // Headers whose tables are empty and link to themselves copy nothing, but each walks 1,761
    // tables before it gives up: the tables are charged as work too.
    let limit = 8 << 20;
    let img = empty_table_loops(200);
    let vol = read_ofs_within(&img, limit).unwrap();
    let reads = failed_reads(&vol.warnings, "broken extension chain");
    assert!(vol.limit_reached, "{:?}", vol.warnings);
    assert!(
        reads * 1760 * 512 <= limit,
        "{reads} reads walked {} tables under a limit of {limit}",
        reads * 1761
    );
}

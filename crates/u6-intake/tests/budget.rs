//! The intake's total output is bounded: synthetic inputs that would make it produce far more
//! than they hold (PP20 files, disk images, volumes whose files share blocks, loose files) stop
//! at the budget. A counting allocator measures what the intake allocates on the test's thread.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::ofs_builder::{ImageBuilder, Node};
use common::pp20_encoder::max_ratio_pp20;
use u6_intake::{InputFile, Intake, intake, read_ofs};

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

#[test]
fn disk_images_past_the_budget_are_not_decoded() {
    // 76 blank disks would come to 68,485,120 bytes; 74 fit.
    let inputs: Vec<InputFile> = (0..76)
        .map(|i| file(format!("disk {i}.hfe"), empty_hfe()))
        .collect();
    let (r, peak) = peak_of(|| intake(inputs));
    assert!(
        peak <= BOUND,
        "the intake held {peak} bytes at once, over {BOUND}"
    );
    let out = output_bytes(&r);
    assert!(out <= BOUND, "the intake kept {out} bytes, over {BOUND}");
    assert_eq!(
        lines_of(&r, "disk 74.hfe"),
        ["disk 74.hfe: not read: 901120 more bytes would pass the intake's 64 MiB budget"]
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

const BS: usize = 512;

fn w32(img: &mut [u8], block: usize, off: usize, v: u32) {
    img[block * BS + off..block * BS + off + 4].copy_from_slice(&v.to_be_bytes());
}

fn checksum(img: &mut [u8], block: usize) {
    w32(img, block, 20, 0);
    let sum = (0..BS).step_by(4).fold(0u32, |s, i| {
        let at = block * BS + i;
        s.wrapping_add(u32::from_be_bytes(img[at..at + 4].try_into().unwrap()))
    });
    w32(img, block, 20, sum.wrapping_neg());
}

/// An FFS volume (FFS data blocks name no owner) whose `count` file headers, chained from the
/// root, all point at the one data block 883 and are their own extension blocks, so each reads
/// `size` bytes out of a single block of the image.
fn shared_block_volume(count: usize, size: u32) -> Vec<u8> {
    // The builder lays out root 880, the header of "a" at 882 and its data block at 883.
    let mut img = ImageBuilder::new(true).build("SHARED", &[Node::File("a", vec![0x33; BS])]);
    let mut prev = 882;
    for k in 0..count {
        let hdr = 884 + k;
        w32(&mut img, hdr, 0, 2); // header
        w32(&mut img, hdr, 4, hdr as u32);
        w32(&mut img, hdr, 8, 72); // a full table of data block pointers
        for i in 0..72 {
            w32(&mut img, hdr, 24 + i * 4, 883);
        }
        w32(&mut img, hdr, 324, size);
        let name = format!("s{k:03}");
        img[hdr * BS + 432] = name.len() as u8;
        img[hdr * BS + 433..hdr * BS + 433 + name.len()].copy_from_slice(name.as_bytes());
        w32(&mut img, hdr, BS - 8, hdr as u32); // the extension: itself
        w32(&mut img, hdr, BS - 4, (-3i32) as u32); // a file
        checksum(&mut img, hdr);
        w32(&mut img, prev, BS - 16, hdr as u32); // the hash chain
        checksum(&mut img, prev);
        prev = hdr;
    }
    img
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
            &"shared.adf: volume SHARED: the next file would pass the intake's 64 MiB budget; \
              it and the files after it are not read"
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

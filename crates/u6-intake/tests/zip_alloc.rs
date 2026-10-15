//! A deflated entry that inflates to far more than it declares: reading it whole or in slices
//! stops at the declared size, and a counting allocator shows what the read held at once stays
//! near that size (synthetic bytes).
//!
//! What the counter measures: the bytes the code asked the global allocator for on the test's
//! own thread, live at once during the call (the sizes requested, not what the allocator rounds
//! them up to; allocations of other threads not counted). It is not the process's memory: no
//! resident size, no allocator overhead or fragmentation, no memory freed on another thread.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use common::zip_writer::zip;
use u6_intake::zip::{Directory, Rebuilt, directory, read_zip, tail_start};

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
/// was allocated before it ran (its inputs).
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let out = f();
    let peak = PEAK.with(Cell::get) - start;
    (out, peak.max(0) as usize)
}

fn disks(name: &str) -> bool {
    name.ends_with(".adf")
}

/// The zip read in slices as the page reads it, then the zip of its slices read.
fn sliced(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, u6_intake::Error> {
    let size = data.len() as u64;
    let mut from = tail_start(size);
    let slices = loop {
        match directory(size, from, &data[from as usize..], disks)? {
            Directory::Need(at) => from = at,
            Directory::Entries(slices) => break slices,
        }
    };
    let mut rebuilt = Rebuilt::for_slices(&slices);
    for s in &slices {
        rebuilt.add(s, &data[s.start as usize..s.end as usize])?;
    }
    read_zip(&rebuilt.finish()?, disks)
}

#[test]
fn inflating_stops_at_the_declared_size_and_holds_about_that_much() {
    let plain = vec![0u8; 64 * MIB];
    let mut data = zip(&[("Disk 1.adf", &plain)], true);
    drop(plain);
    // The central record declares 1 MiB unpacked (offset 24 in the record).
    let e = data.len() - 22;
    let at = u32::from_le_bytes(data[e + 16..e + 20].try_into().unwrap()) as usize;
    data[at + 24..at + 28].copy_from_slice(&(MIB as u32).to_le_bytes());
    assert!(data.len() < MIB, "the entry packs 64 MiB into little");
    // The output grows by doubling up to the declared size: at its last step the allocator may
    // hold the old block and a new one of up to twice the size (3 MiB), beside the input's copy
    // in the rebuilt zip. Without the limit it would hold the 64 MiB the stream inflates to.
    let bound = 4 * MIB + 2 * data.len();
    for (path, read) in [
        (
            "whole",
            &(|d: &[u8]| read_zip(d, disks)) as &dyn Fn(&[u8]) -> _,
        ),
        ("sliced", &sliced),
    ] {
        let (out, peak) = peak_of(|| read(&data));
        let err = out.unwrap_err().to_string();
        assert!(
            err.starts_with("Disk 1.adf: inflate failed"),
            "{path}: {err}"
        );
        assert!(
            peak < bound,
            "{path}: {peak} bytes held at once, bound {bound}"
        );
        println!("{path}: the read held at most {peak} bytes at once (bound {bound})");
    }
    // Declared at its real size, it inflates whole, read either way.
    data[at + 24..at + 28].copy_from_slice(&((64 * MIB) as u32).to_le_bytes());
    for out in [read_zip(&data, disks).unwrap(), sliced(&data).unwrap()] {
        assert!(out.len() == 1 && out[0].1.len() == 64 * MIB && out[0].1.iter().all(|&b| b == 0));
    }
}

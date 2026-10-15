//! The codex event reader's peak heap: a counting allocator over the system one, around
//! `read_events` of a stream holding one line of 256 MiB, then lines at the cap. One test, so
//! nothing else allocates while it measures.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};

use u6_hdtool::run::{LINE_MAX, read_events};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        NOW.fetch_sub(layout.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static A: Counting = Counting;

/// `fill` bytes of `byte` and then `tail`, made as they are read (never held).
struct Stream {
    fill: u64,
    byte: u8,
    tail: Vec<u8>,
    at: usize,
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.fill > 0 {
            let n = buf.len().min(self.fill as usize);
            buf[..n].fill(self.byte);
            self.fill -= n as u64;
            return Ok(n);
        }
        let n = buf.len().min(self.tail.len() - self.at);
        buf[..n].copy_from_slice(&self.tail[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// The peak heap above the start while `read_events` reads `s`, and the ids it reported.
fn measure(s: Stream) -> (usize, Vec<String>) {
    let mut ids = Vec::with_capacity(4);
    let base = NOW.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    read_events(s, std::io::sink(), |id| ids.push(id));
    (PEAK.load(Ordering::SeqCst) - base, ids)
}

#[test]
fn the_reader_never_holds_more_than_a_line() {
    let thread = b"\n{\"type\":\"thread.started\",\"thread_id\":\"t-1\"}\n".to_vec();
    let (peak, ids) = measure(Stream {
        fill: 256 << 20,
        byte: b'x',
        tail: thread.clone(),
        at: 0,
    });
    println!("peak {peak} bytes reading a 256 MiB line");
    assert_eq!(ids, ["t-1"]);
    assert!(peak <= 4 * LINE_MAX, "peak {peak}");
    // Lines at the cap, each parsed as JSON and kept: still a few lines' worth.
    let one = format!("{{\"pad\":\"{}\"}}\n", "y".repeat(LINE_MAX - 10));
    let mut tail = one.repeat(64).into_bytes();
    tail.extend_from_slice(&thread);
    let (peak, ids) = measure(Stream {
        fill: 0,
        byte: 0,
        tail,
        at: 0,
    });
    println!("peak {peak} bytes reading 64 lines at the cap");
    assert_eq!(ids, ["t-1"]);
    assert!(peak <= 4 * LINE_MAX, "peak {peak}");
}

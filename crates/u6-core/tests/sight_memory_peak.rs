//! Spec 18's peak heap of the remembered sight, measured with a counting allocator over the
//! system one (as `u6-web`'s `pack_memory`). On the owner's game: the party put (0x1092fc) on the
//! surface cell after cell of a grid a view apart, the view rebuilt (0x109e74), the animation
//! ticked (0x10202e: the shore merge rewrites its records, so the copy store fills as in play)
//! and the window rendered framed with the remembered sight on after each (0x102c90). The cells
//! come to the cap, the cap acts again and again, and the cells' table grows once from 2^18 to
//! 2^19 buckets. What the memory retains is under 16 MiB (`the_bounds_hold_the_heap_under_16_mib`);
//! the growth is the peak: for a moment the old table and the new live together. Prints counts
//! only; one test, so nothing else allocates while it measures. Runs only where the owner's zip
//! exists (never in CI: the data is not ours to ship).
#![allow(clippy::disallowed_methods)] // reads the owner's zip

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use u6_core::geom::{VIEW_H, VIEW_W};
use u6_core::{Engine, Files, State};
use u6_intake::{InputFile, intake};
use u6_testkit::Quiet;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// The largest single allocation since the last reset.
static LARGEST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
            LARGEST.fetch_max(layout.size(), Ordering::SeqCst);
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

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// The bytes of a cells' table of `buckets` buckets in one allocation (std's hashbrown): an
/// entry of 20 bytes and a control byte per bucket, and a group's width of control bytes more
/// (at most 16).
fn table(buckets: usize) -> usize {
    buckets * 21
}

/// Spec 18 (the review's peak): the walk above, until the cells' table has grown to 2^19 buckets
/// and 200 renders more (the cap acting after them). Measured over the heap at the first render
/// (the game state, the engine's buffers): the peak and what is retained at the end. The table
/// grew once to 2^19 buckets and never to 2^20, and the peak stays under 24 MiB: the two tables
/// of the growth (16.5 MB), the copies, the cap's scratch and the renders' own.
#[test]
fn the_remembered_sight_peaks_once_as_its_table_grows() {
    let Some(files) = files() else { return };
    let mut s = State::new_game(&files).quiet();
    u6_core::main_loop::load_state_tail(&mut s);
    let e = Engine::with_game(State::empty(), Files::default(), |_, _| async {});
    let io = e.io();
    io.set_sight_memory(true);
    u6_core::gfx::render_window(&mut s, io, true);
    let base = NOW.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    LARGEST.store(0, Ordering::SeqCst);
    let (mut renders, mut grown, mut most) = (0usize, None, 0);
    let grid = (0..1024u16)
        .step_by(VIEW_H)
        .flat_map(|y| (0..1024u16).step_by(VIEW_W).map(move |x| (x, y)));
    for (x, y) in grid {
        u6_core::map::travel::teleport(&mut s, x, y, 0, 0);
        u6_core::map::view::update_view(&mut s);
        u6_core::gfx::anim::anim_tick(&mut s);
        u6_core::gfx::render_window(&mut s, io, true);
        renders += 1;
        most = most.max(s.gfx.cache.sight_cells());
        if grown.is_none() && LARGEST.load(Ordering::SeqCst) >= table(1 << 19) {
            grown = Some(renders);
        }
        if grown.is_some_and(|g| renders >= g + 200) {
            break;
        }
    }
    let peak = PEAK.load(Ordering::SeqCst) - base;
    let retained = NOW.load(Ordering::SeqCst).saturating_sub(base);
    let largest = LARGEST.load(Ordering::SeqCst);
    println!(
        "{renders} renders, the table grown at render {grown:?}; at most {most} cells, {} at the \
         end; largest allocation {largest} bytes; peak {peak} bytes over the start, {retained} \
         retained at the end",
        s.gfx.cache.sight_cells()
    );
    assert!(grown.is_some(), "the table grew to 2^19 buckets");
    assert!(largest < table(1 << 20), "never to 2^20 buckets");
    assert!(
        most <= u6_core::gfx::cache::MAX_CELLS,
        "at most MAX_CELLS cells"
    );
    assert!(peak < 24 << 20, "the peak under 24 MiB");
}

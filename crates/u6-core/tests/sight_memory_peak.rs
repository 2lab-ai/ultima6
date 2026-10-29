//! Spec 18's peak heap of the remembered sight, measured with a counting allocator over the
//! system one (as `u6-web`'s `pack_memory`). On the owner's game: the party put (0x1092fc) on the
//! surface cell after cell of a grid a view apart, the view rebuilt (0x109e74), the animation
//! ticked (0x10202e: the shore merge rewrites its records, so the copy store fills as in play)
//! and the window rendered framed with the remembered sight on after each (0x102c90). The cells
//! come to the cap, the cap acts again and again, and the cells' table grows once from 2^18 to
//! 2^19 buckets. What the memory retains is under 16 MiB (`the_bounds_hold_the_heap_under_16_mib`);
//! the growth is the peak: for a moment the old table and the new live together. Then the file the
//! memory is saved as (spec 18, saved with the game): a Save, a second Save and a Restore of a
//! memory at the cap. Prints counts only; the tests take one lock ([`serial`]), so nothing else
//! allocates while one measures. Runs only where the owner's zip exists (never in CI: the data is
//! not ours to ship); without it each test skips, unless `U6_REQUIRE_ARCHIVE=1` (the release
//! receipt), which fails it instead. Each prints `sight_memory_peak: ran` or
//! `sight_memory_peak: skipped`, which a gate log shows when the tests run with `--nocapture`.
#![allow(clippy::disallowed_methods)] // reads the owner's zip

use std::alloc::{GlobalAlloc, Layout, System};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use u6_core::geom::{VIEW_H, VIEW_W};
use u6_core::{Engine, Files, State};
use u6_intake::{InputFile, intake};
use u6_testkit::Quiet;

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// The largest single allocation since the last reset.
static LARGEST: AtomicUsize = AtomicUsize::new(0);
/// The allocations of a cells' table of 2^19 buckets ([`is_table`]), and of 2^20 or more.
static TABLES_19: AtomicUsize = AtomicUsize::new(0);
static TABLES_20: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
            LARGEST.fetch_max(layout.size(), Ordering::SeqCst);
            if is_table(layout.size(), 1 << 19) {
                TABLES_19.fetch_add(1, Ordering::SeqCst);
            } else if layout.size() >= table(1 << 20) {
                TABLES_20.fetch_add(1, Ordering::SeqCst);
            }
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

/// The counters are the process's: one test measures at a time (a failed one does not stop the
/// next).
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn files() -> Option<Files> {
    if !u6_testkit::archive().exists() {
        assert_ne!(
            std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
            Ok("1"),
            "sight_memory_peak: U6_REQUIRE_ARCHIVE=1 and the archive is missing"
        );
        println!("sight_memory_peak: skipped (the archive (U6_ARCHIVE) not present)");
        return None;
    }
    let data = std::fs::read(u6_testkit::archive()).quiet();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

/// The bytes of a cells' table of `buckets` buckets in one allocation (std's hashbrown), but the
/// group's width of control bytes more: an entry of 20 bytes and a control byte per bucket.
fn table(buckets: usize) -> usize {
    buckets * 21
}

/// Whether an allocation of `size` bytes is a cells' table of `buckets` buckets: [`table`] and a
/// group's width of control bytes (8 with NEON or on wasm32, 16 with SSE2), aligned to it.
fn is_table(size: usize, buckets: usize) -> bool {
    (table(buckets) + 8..=table(buckets) + 16).contains(&size)
}

/// The copy store as in play: its size is a constant of the game data, 255 copies on the owner's
/// game (`real_the_copies_of_the_merged_records_are_a_constant_of_the_game_data`), each under
/// 1 KiB with its index.
const COPIES: usize = 255 * 1024;

/// Whether std's hashbrown groups its control bytes by 8 on this target, as on aarch64 (NEON)
/// and wasm32 (the generic 64-bit word); false on x86 and x86_64 (SSE2's 16 bytes) and on every
/// target not considered here, as `u6_core::gfx::cache`'s tests define it. The 2^18 → 2^19
/// growth under the cap rests on deleted entries piling up; with 16-byte groups they form far
/// less often, and the growth, or a cap and 200 renders after it, may not come before the grid
/// ends. Not run with other widths than 8.
const EIGHT_BYTE_GROUPS: bool = cfg!(any(target_arch = "aarch64", target_arch = "wasm32"));

/// Spec 18 (the review's peak): the walk above, until the cells' table has grown to 2^19 buckets,
/// 200 renders more have passed and the cap has acted after the growth (the cells held fell from
/// one render to the next: in Original a render adds at most 121 new cells, so 200 renders may
/// hold no cap). Measured over the heap at the first render (the game state, the engine's
/// buffers): the peak and what is retained at the end.
///
/// Only the cap lowers the cells held in this walk, each way of the others asserted:
/// - the sweep: the clock stands still (the teleport, 0x1092fc, ticks the clock, 0x10368a, by 0
///   minutes), so no cell ages and the sweep forgets none: the calendar the same after every
///   render;
/// - a fall in a render where the cap does not act (a load state's reset among them, which
///   leaves at most a render's cells): every fall in the cells held lands on exactly seven
///   eighths of the cap, 200 704, where the cap always lands (the oldest first, ties by key).
///   The cap acts above 229 255 cells in Original (228 599 in Wide), so its drop is far more than
///   a render adds;
/// - `forget` (a cell's look not kept, the copy store full): it needs a full store, 8 192 copies
///   (`MAX_COPIES`, about 5.4 MB), which the store, never freed, would still hold at the end; the
///   heap retained at the end is at most the 2^19 table, the copies as in play ([`COPIES`]: 255,
///   a constant of the game data) and 1 MiB, 12 319 744 bytes [computed], no room for it.
///
/// These run with every width of control groups.
///
/// Then:
/// - The table grew to 2^19 buckets once (one allocation of that table's size) and never to
///   2^20; 200 renders or more passed after the growth, a cap among them.
/// - The high-water was set at the render of that growth, and no later render passed it, the
///   cap's after the growth included.
/// - The peak is at least the two tables of the growth (2^18 and 2^19 buckets, 16.5 MB: for a
///   moment both live) and at most those, the copies and 1 MiB (the renders' own and the cap's
///   scratch).
///
/// With other widths than 8 ([`EIGHT_BYTE_GROUPS`]), a walk that does not reach the growth, a cap
/// and 200 renders past it before its grid ends waives the asserts of this list but "never to
/// 2^20"; one that reaches them asserts them all.
#[test]
fn the_remembered_sight_peaks_once_as_its_table_grows() {
    let _one = serial();
    let Some(files) = files() else { return };
    println!("sight_memory_peak: ran");
    let mut s = State::new_game(&files).quiet();
    u6_core::main_loop::load_state_tail(&mut s);
    let e = Engine::with_game(State::empty(), Files::default(), |_, _| async {});
    let io = e.io();
    io.set_sight_memory(true);
    u6_core::gfx::render_window(&mut s, io, true);
    let base = NOW.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    LARGEST.store(0, Ordering::SeqCst);
    TABLES_19.store(0, Ordering::SeqCst);
    TABLES_20.store(0, Ordering::SeqCst);
    let (mut renders, mut grown, mut capped, mut held) = (0usize, None, false, 0);
    let calendar = s.actors.calendar;
    let after_cap = u6_core::gfx::cache::MAX_CELLS - u6_core::gfx::cache::MAX_CELLS / 8;
    let grid = (0..1024u16)
        .step_by(VIEW_H)
        .flat_map(|y| (0..1024u16).step_by(VIEW_W).map(move |x| (x, y)));
    for (x, y) in grid {
        let (tables, before) = (
            TABLES_19.load(Ordering::SeqCst),
            PEAK.load(Ordering::SeqCst),
        );
        u6_core::map::travel::teleport(&mut s, x, y, 0, 0);
        u6_core::map::view::update_view(&mut s);
        u6_core::gfx::anim::anim_tick(&mut s);
        u6_core::gfx::render_window(&mut s, io, true);
        renders += 1;
        if grown.is_none() && TABLES_19.load(Ordering::SeqCst) > tables {
            grown = Some((renders, before, PEAK.load(Ordering::SeqCst)));
        }
        assert!(s.actors.calendar == calendar, "the clock stands still");
        let cells = s.gfx.cache.sight_cells();
        if cells < held {
            assert_eq!(cells, after_cap, "a fall in the cells held is the cap's");
        }
        capped |= grown.is_some_and(|(g, _, _)| renders > g) && cells < held;
        held = cells;
        if capped && grown.is_some_and(|(g, _, _)| renders >= g + 200) {
            break;
        }
    }
    let peak = PEAK.load(Ordering::SeqCst) - base;
    let retained = NOW.load(Ordering::SeqCst).saturating_sub(base);
    let largest = LARGEST.load(Ordering::SeqCst);
    let (tables, larger) = (
        TABLES_19.load(Ordering::SeqCst),
        TABLES_20.load(Ordering::SeqCst),
    );
    assert!(
        retained <= table(1 << 19) + COPIES + (1 << 20),
        "retained at the end: the 2^19 table, the copies as in play and 1 MiB (no full store)"
    );
    let reached = grown.is_some_and(|(g, _, _)| capped && renders >= g + 200);
    if !EIGHT_BYTE_GROUPS && !reached {
        println!(
            "{renders} renders, the growth to 2^19 buckets with a cap and 200 renders after it \
             not reached (not 8-byte groups: waived); peak {peak} bytes over the start"
        );
        assert_eq!(larger, 0, "never to 2^20 buckets");
        return;
    }
    assert!(grown.is_some(), "the table grew to 2^19 buckets");
    let (at, before, after) = grown.quiet();
    println!(
        "{renders} renders; the 2^19 table allocated {tables} times, first at render {at} (the \
         high-water {} bytes over the start before it, {} after it); largest allocation \
         {largest} bytes; peak {peak} bytes over the start, {retained} retained at the end, {} \
         cells",
        before - base,
        after - base,
        s.gfx.cache.sight_cells()
    );
    let low = table(1 << 18) + table(1 << 19);
    let high = low + COPIES + (1 << 20);
    assert_eq!(tables, 1, "the table grew to 2^19 buckets once");
    assert!(capped, "the cap acted after the growth");
    assert!(renders >= at + 200, "200 renders after the growth");
    assert_eq!(larger, 0, "never to 2^20 buckets");
    assert!(after > before, "the growth's render raised the high-water");
    assert_eq!(after - base, peak, "and no later render passed it");
    assert!(
        peak >= low,
        "the peak: at least the two tables of the growth"
    );
    assert!(
        peak <= high,
        "the peak: at most the tables, the copies and 1 MiB"
    );
}

/// Spec 18 (saved with the game): a sight file of version 1 bound to an empty objlist (its
/// length 0, the FNV-1a 64 digest of no bytes): `copies` copies of record 9 of distinct contents
/// (pixels 0 and 1 the slot's two bytes, the rest 0, opaque), then `cells` cells of one layer on
/// the surface, keys 0, 1, .. (x the key's bits 10 on, y its low 10), seen at `minute`, record 1,
/// every eighth layer drawn from a copy.
fn sight_file(copies: usize, cells: usize, minute: u32) -> Vec<u8> {
    file_of(copies, minute, (0..cells as u32).map(|k| (k, 1)))
}

/// [`sight_file`]'s copies and minute, then a cell for each (key, layer count) of `cells` (the
/// keys increasing), its layers all of record 1, cell n's first drawn from a copy when n is a
/// multiple of 8 (slot n / 8, round the copies), the others from none.
fn file_of(copies: usize, minute: u32, cells: impl Iterator<Item = (u32, u16)> + Clone) -> Vec<u8> {
    const NO_BYTES: u64 = 0xcbf2_9ce4_8422_2325;
    let (count, layers) =
        (cells.clone()).fold((0, 0), |(n, l), (_, c)| (n + 1, l + usize::from(c)));
    let mut f = Vec::with_capacity(44 + 290 * copies + 10 * count + 6 * layers);
    f.extend_from_slice(b"u6sight\0");
    f.extend_from_slice(&1u32.to_le_bytes());
    f.extend_from_slice(&0u32.to_le_bytes());
    f.extend_from_slice(&NO_BYTES.to_le_bytes());
    f.extend_from_slice(&u64::MAX.to_le_bytes());
    for n in [copies, count, layers] {
        f.extend_from_slice(&(n as u32).to_le_bytes());
    }
    for i in 0..copies {
        f.extend_from_slice(&9u16.to_le_bytes());
        let mut pixels = [0u8; 256];
        pixels[..2].copy_from_slice(&(i as u16).to_le_bytes());
        f.extend_from_slice(&pixels);
        f.extend_from_slice(&[0u8; 32]);
    }
    for (n, (k, count)) in cells.enumerate() {
        f.extend_from_slice(&k.to_le_bytes());
        f.extend_from_slice(&minute.to_le_bytes());
        f.extend_from_slice(&count.to_le_bytes());
        for l in 0..count {
            f.extend_from_slice(&1u16.to_le_bytes());
            f.extend_from_slice(&[0, 0]);
            let slot = match (l, n % 8) {
                (0, 0) => (n / 8 % copies) as u16,
                _ => u16::MAX,
            };
            f.extend_from_slice(&slot.to_le_bytes());
        }
    }
    f
}

/// The u32 at byte `at` of sight file `file` (its header's counts: the copies at 32, the cells at
/// 36, the layers at 40).
fn header(file: &[u8], at: usize) -> usize {
    let n: [u8; 4] = file.get(at..at + 4).and_then(|b| b.try_into().ok()).quiet();
    u32::from_le_bytes(n) as usize
}

/// The game clock of `s` in minutes, as the remembered sight keeps it (spec 18: 60 minutes, 24
/// hours, days 1..=28, months 1..=12), so a cell seen then is of day 0.
fn minute(s: &State) -> u32 {
    let c = s.actors.calendar;
    let months = u64::from(c.year) * 12 + u64::from(c.month.saturating_sub(1));
    let days = months * 28 + u64::from(c.day.saturating_sub(1));
    let now = (days * 24 + u64::from(c.hour)) * 60 + u64::from(c.minute);
    u32::try_from(now).quiet()
}

/// The peak of `run` on `s` over the heap before it, and the bytes it left held (or let go).
fn measure(s: &mut State, run: impl FnOnce(&mut State)) -> (usize, isize) {
    let before = NOW.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    run(s);
    let peak = PEAK.load(Ordering::SeqCst) - before;
    let held = NOW.load(Ordering::SeqCst) as isize - before as isize;
    (peak, held)
}

/// Spec 18 (saved with the game, Bounds): a memory at the cap, its most cells after a render
/// (`MAX_CELLS` less a render's) of one layer each and `MAX_COPIES` copies (the copy store full),
/// on the owner's game state, saved twice and then restored. Each step's peak over the heap
/// before it, beside the memory's own heap and its file:
/// - a Save holds the memory, the file it writes and the keys it writes them in (4 bytes a cell);
///   the last Save's file is let go first, so a second Save peaks at no more than the first;
/// - a Restore lets the memory go before it reads the file's (load state forgets it first), so it
///   peaks at the larger of the two memories, not at both.
///
/// Each within 64 KiB of that (the Save's and the Restore's own work: the objlist, the region, 26
/// KB with nothing remembered). Before the Restore the memory is made another of the same size
/// (every cell a minute older), and after it the memory is the saved one again, cell for cell,
/// layer for layer and copy for copy (written again, it is the saved file's bytes): so the
/// Restore read the file, all of it, and did not keep the memory it found. Both outside the
/// measures.
#[test]
fn a_save_and_a_restore_of_a_full_memory_hold_one_memory_and_one_file() {
    let _one = serial();
    let Some(files) = files() else { return };
    println!("sight_memory_peak: ran");
    let mut s = State::new_game(&files).quiet();
    u6_core::main_loop::load_state_tail(&mut s);
    let e = Engine::with_game(State::empty(), files, |_, _| async {});
    let io = e.io();
    io.set_sight_memory(true);
    let (bare_save, _) = measure(&mut s, u6_core::saveload::save);
    let (bare_restore, _) = measure(&mut s, |s| u6_core::saveload::restore(s, io).quiet());
    let cells = u6_core::gfx::cache::MAX_CELLS - VIEW_W * VIEW_H;
    let copies = u6_core::gfx::cache::MAX_COPIES;
    let start = NOW.load(Ordering::SeqCst);
    let file = sight_file(copies, cells, minute(&s));
    let memory = u6_core::gfx::cache::read_sight(&file, &[]).quiet();
    drop(file);
    s.gfx.cache.set_sight(memory);
    let own = NOW.load(Ordering::SeqCst) - start;
    assert_eq!(s.gfx.cache.sight_cells(), cells, "the memory at the cap");
    let saved = |s: &State| s.saveload.sight.as_ref().map_or(0, |f| f.bytes().len());
    let (first, first_held) = measure(&mut s, u6_core::saveload::save);
    let size = saved(&s);
    let (second, second_held) = measure(&mut s, u6_core::saveload::save);
    assert_eq!(saved(&s), size, "the same file again");
    let objlist = s.saveload.objlist.clone().quiet();
    let file = s.saveload.sight.clone().quiet();
    let older = sight_file(copies, cells, minute(&s) - 1);
    s.gfx
        .cache
        .set_sight(u6_core::gfx::cache::read_sight(&older, &[]).quiet());
    drop(older);
    assert!(
        s.gfx
            .cache
            .sight_file(&objlist)
            .is_some_and(|m| m != file.bytes()),
        "before the Restore: another memory of the same size"
    );
    let (restore, restore_held) = measure(&mut s, |s| u6_core::saveload::restore(s, io).quiet());
    assert_eq!(
        s.gfx.cache.sight_cells(),
        cells,
        "the Restore reads it back"
    );
    assert!(
        s.gfx.cache.sight_file(&objlist).as_deref() == Some(file.bytes()),
        "the Restore reads back the saved memory: its cells, their layers and records, its copies"
    );
    let keys = 4 * cells;
    println!(
        "a memory of {cells} cells and {copies} copies: {own} bytes its own, a file of {size} \
         bytes, the keys {keys}; nothing remembered: a Save peaks {bare_save} bytes over the heap \
         before it, a Restore {bare_restore}; the first Save {first} ({first_held} held after), \
         the second {second} ({second_held}), the Restore {restore} ({restore_held})"
    );
    let slack = 64 << 10;
    assert!(
        first <= size + keys + slack,
        "a Save: the file, its keys and 64 KiB over the heap before it"
    );
    assert!(
        second <= keys + slack,
        "a second Save: the last Save's file let go first"
    );
    assert!(
        restore <= slack,
        "a Restore: the old memory let go before the new one is read"
    );
}

/// Spec 18 (saved with the game, Bounds, past [`u6_core::gfx::cache::MAX_SAVED_LAYERS`]: never in
/// play): a memory of more layers than a file holds and `MAX_COPIES` copies, on the owner's game
/// state, saved. Its cells: a third of `MAX_SAVED_LAYERS` of three layers each and one for the
/// rest, so their layers are exactly that limit, on the dungeon levels (keys above every surface
/// key), read from a file; then one framed render of the party's view puts the cells in sight on
/// top, past the limit, of the same minute (the clock stands still). The Save writes the newest
/// sights that fit (of one minute, the highest keys): the file's cells come first and fill the
/// limit exactly, and the first of the render's (a layer or more) does not fit, so the file holds
/// every cell read from the file and none of the render's (its cells the cells read, its layers
/// the limit). Outside the measure, the file the Save wrote is decoded against the Save's objlist
/// and compared with the fixture's from the counts on: its copies, and its cells' keys, minutes,
/// layer counts and layers. To choose the cells the Save sorts them by sight: besides the keys (4
/// bytes a cell) it collects a tuple of 16 bytes a cell in a vector grown by doubling from 4, and
/// lets it go before it makes the file; at the vector's last growth the old block and the new
/// live together, 3 × 16 bytes × the largest power of two under the cells. Its peak over the heap
/// before it: at most the keys, the larger of the file and those tuples, and 64 KiB.
#[test]
fn a_save_past_the_files_layers_holds_the_keys_and_the_sort_or_the_file() {
    let _one = serial();
    let Some(files) = files() else { return };
    println!("sight_memory_peak: ran");
    let mut s = State::new_game(&files).quiet();
    u6_core::main_loop::load_state_tail(&mut s);
    let e = Engine::with_game(State::empty(), files, |_, _| async {});
    let io = e.io();
    io.set_sight_memory(true);
    let most = u6_core::gfx::cache::MAX_SAVED_LAYERS;
    let copies = u6_core::gfx::cache::MAX_COPIES;
    let stacks = most / 3;
    // Cell n of the dungeon levels from level 1 on, 256 × 256 cells a level.
    let below = |n: usize| ((1 + n / 0x1_0000) << 20 | (n >> 8 & 0xff) << 10 | n & 0xff) as u32;
    let layers = |n: usize| {
        if n < stacks {
            3
        } else {
            (most - 3 * stacks) as u16
        }
    };
    let now = minute(&s);
    let fixture = || file_of(copies, now, (0..=stacks).map(|n| (below(n), layers(n))));
    let file = fixture();
    s.gfx
        .cache
        .set_sight(u6_core::gfx::cache::read_sight(&file, &[]).quiet());
    drop(file);
    let read = s.gfx.cache.sight_cells();
    u6_core::gfx::render_window(&mut s, io, true);
    let cells = s.gfx.cache.sight_cells();
    assert!(
        read == stacks + 1 && cells > read,
        "the render's cells on top of the file's"
    );
    let (save, held) = measure(&mut s, u6_core::saveload::save);
    let saved = s.saveload.sight.clone().quiet();
    let size = saved.bytes().len();
    let (kept, written) = (header(saved.bytes(), 36), header(saved.bytes(), 40));
    assert!(
        kept == read && written == most,
        "past the file's layers: the cells read from the file, all of them, none of the render's"
    );
    let expected = fixture();
    u6_testkit::assert_same!(
        saved.bytes()[32..],
        expected[32..],
        "the file from its counts on: the fixture's copies and cells"
    );
    drop(expected);
    let objlist = s.saveload.objlist.clone().quiet();
    let back = u6_core::gfx::cache::read_sight(saved.bytes(), &objlist);
    assert!(back.is_ok(), "the file decodes against the Save's objlist");
    s.gfx.cache.set_sight(back.quiet());
    assert!(s.gfx.cache.sight_cells() == read, "into the cells read");
    let keys = 4 * cells;
    let tuples = (3 * std::mem::size_of::<(u32, u32, usize)>()) << (cells - 1).ilog2();
    println!(
        "past the file's layers: a memory of {cells} cells and {copies} copies, a file of {size} \
         bytes of {kept} cells and {written} layers, the keys {keys}, the sort's tuples at their \
         last growth {tuples}; the Save peaks {save} bytes over the heap before it ({held} held \
         after)"
    );
    assert!(
        save <= keys + size.max(tuples) + (64 << 10),
        "a Save past the file's layers: the keys, the larger of the file and the tuples at their \
         last growth, and 64 KiB"
    );
}
